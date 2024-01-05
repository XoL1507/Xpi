// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use anyhow::anyhow;
use backoff::future::retry;
use bytes::Bytes;
use futures::StreamExt;
use object_store::path::Path;
use object_store::DynObjectStore;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, warn};
use url::Url;

pub async fn get(location: &Path, from: Arc<DynObjectStore>) -> Result<Bytes, object_store::Error> {
    let backoff = backoff::ExponentialBackoff::default();
    let bytes = retry(backoff, || async {
        from.get(location).await.map_err(|e| {
            error!("Failed to read file from object store with error: {:?}", &e);
            backoff::Error::transient(e)
        })
    })
    .await?
    .bytes()
    .await?;
    Ok(bytes)
}

pub async fn put(
    location: &Path,
    bytes: Bytes,
    to: Arc<DynObjectStore>,
) -> Result<(), object_store::Error> {
    let backoff = backoff::ExponentialBackoff::default();
    retry(backoff, || async {
        if !bytes.is_empty() {
            to.put(location, bytes.clone()).await.map_err(|e| {
                error!("Failed to write file to object store with error: {:?}", &e);
                backoff::Error::transient(e)
            })
        } else {
            warn!("Not copying empty file: {:?}", location);
            Ok(())
        }
    })
    .await?;
    Ok(())
}

pub async fn copy_file(
    path_in: Path,
    path_out: Path,
    from: Arc<DynObjectStore>,
    to: Arc<DynObjectStore>,
) -> Result<(), object_store::Error> {
    let bytes = from.get(&path_in).await?.bytes().await?;
    if !bytes.is_empty() {
        put(&path_out, bytes, to).await
    } else {
        warn!("Not copying empty file: {:?}", path_in);
        Ok(())
    }
}

pub async fn copy_files(
    files_in: &[Path],
    files_out: &[Path],
    from: Arc<DynObjectStore>,
    to: Arc<DynObjectStore>,
    concurrency: NonZeroUsize,
) -> Result<Vec<()>, object_store::Error> {
    let results: Vec<Result<(), object_store::Error>> =
        futures::stream::iter(files_in.iter().zip(files_out.iter()))
            .map(|(path_in, path_out)| {
                copy_file(path_in.clone(), path_out.clone(), from.clone(), to.clone())
            })
            .boxed()
            .buffer_unordered(concurrency.get())
            .collect()
            .await;
    results.into_iter().collect()
}

pub async fn copy_recursively(
    dir: &Path,
    from: Arc<DynObjectStore>,
    to: Arc<DynObjectStore>,
    concurrency: NonZeroUsize,
) -> Result<Vec<()>, object_store::Error> {
    let mut input_paths = vec![];
    let mut output_paths = vec![];
    let mut paths = from.list(Some(dir)).await?;
    while let Some(res) = paths.next().await {
        if let Ok(object_metadata) = res {
            input_paths.push(object_metadata.location.clone());
            output_paths.push(object_metadata.location);
        } else {
            return Err(res.err().unwrap());
        }
    }
    copy_files(
        &input_paths,
        &output_paths,
        from.clone(),
        to.clone(),
        concurrency,
    )
    .await
}

pub async fn delete_files(
    files: &[Path],
    store: Arc<DynObjectStore>,
    concurrency: NonZeroUsize,
) -> Result<Vec<()>, object_store::Error> {
    let results: Vec<Result<(), object_store::Error>> = futures::stream::iter(files)
        .map(|f| {
            let backoff = backoff::ExponentialBackoff::default();
            retry(backoff, || async {
                store.clone().delete(f).await.map_err(|e| {
                    error!("Failed to delete file on object store with error: {:?}", &e);
                    backoff::Error::transient(e)
                })
            })
        })
        .boxed()
        .buffer_unordered(concurrency.get())
        .collect()
        .await;
    results.into_iter().collect()
}

pub async fn delete_recursively(
    path: &Path,
    store: Arc<DynObjectStore>,
    concurrency: NonZeroUsize,
) -> Result<Vec<()>, object_store::Error> {
    let mut paths_to_delete = vec![];
    let mut paths = store.list(Some(path)).await?;
    while let Some(res) = paths.next().await {
        if let Ok(object_metadata) = res {
            paths_to_delete.push(object_metadata.location);
        } else {
            return Err(res.err().unwrap());
        }
    }
    delete_files(&paths_to_delete, store.clone(), concurrency).await
}

pub fn path_to_filesystem(local_dir_path: PathBuf, location: &Path) -> anyhow::Result<PathBuf> {
    // Convert an `object_store::path::Path` to `std::path::PathBuf`
    let path = std::fs::canonicalize(local_dir_path)?;
    let mut url = Url::from_file_path(&path)
        .map_err(|_| anyhow!("Failed to parse input path: {}", path.display()))?;
    url.path_segments_mut()
        .map_err(|_| anyhow!("Failed to get path segments: {}", path.display()))?
        .pop_if_empty()
        .extend(location.parts());
    let new_path = url
        .to_file_path()
        .map_err(|_| anyhow!("Failed to convert url to path: {}", url.as_str()))?;
    Ok(new_path)
}

#[cfg(test)]
mod tests {
    use crate::object_store::util::{copy_recursively, delete_recursively};
    use crate::object_store::{ObjectStoreConfig, ObjectStoreType};
    use object_store::path::Path;
    use std::fs;
    use std::num::NonZeroUsize;
    use tempfile::TempDir;

    #[tokio::test]
    pub async fn test_copy_recursively() -> anyhow::Result<()> {
        let input = TempDir::new()?;
        let input_path = input.path();
        let child = input_path.join("child");
        fs::create_dir(&child)?;
        let file1 = child.join("file1");
        fs::write(file1, b"Lorem ipsum")?;
        let grandchild = child.join("grand_child");
        fs::create_dir(&grandchild)?;
        let file2 = grandchild.join("file2");
        fs::write(file2, b"Lorem ipsum")?;

        let output = TempDir::new()?;
        let output_path = output.path();

        let input_store = ObjectStoreConfig {
            object_store: Some(ObjectStoreType::File),
            directory: Some(input_path.to_path_buf()),
            ..Default::default()
        }
        .make()?;

        let output_store = ObjectStoreConfig {
            object_store: Some(ObjectStoreType::File),
            directory: Some(output_path.to_path_buf()),
            ..Default::default()
        }
        .make()?;

        copy_recursively(
            &Path::from("child"),
            input_store,
            output_store,
            NonZeroUsize::new(1).unwrap(),
        )
        .await?;

        assert!(output_path.join("child").exists());
        assert!(output_path.join("child").join("file1").exists());
        assert!(output_path.join("child").join("grand_child").exists());
        assert!(output_path
            .join("child")
            .join("grand_child")
            .join("file2")
            .exists());
        let content = fs::read_to_string(output_path.join("child").join("file1"))?;
        assert_eq!(content, "Lorem ipsum");
        let content =
            fs::read_to_string(output_path.join("child").join("grand_child").join("file2"))?;
        assert_eq!(content, "Lorem ipsum");
        Ok(())
    }

    #[tokio::test]
    pub async fn test_delete_recursively() -> anyhow::Result<()> {
        let input = TempDir::new()?;
        let input_path = input.path();
        let child = input_path.join("child");
        fs::create_dir(&child)?;
        let file1 = child.join("file1");
        fs::write(file1, b"Lorem ipsum")?;
        let grandchild = child.join("grand_child");
        fs::create_dir(&grandchild)?;
        let file2 = grandchild.join("file2");
        fs::write(file2, b"Lorem ipsum")?;

        let input_store = ObjectStoreConfig {
            object_store: Some(ObjectStoreType::File),
            directory: Some(input_path.to_path_buf()),
            ..Default::default()
        }
        .make()?;

        delete_recursively(
            &Path::from("child"),
            input_store,
            NonZeroUsize::new(1).unwrap(),
        )
        .await?;

        assert!(!input_path.join("child").join("file1").exists());
        assert!(!input_path
            .join("child")
            .join("grand_child")
            .join("file2")
            .exists());
        Ok(())
    }
}
