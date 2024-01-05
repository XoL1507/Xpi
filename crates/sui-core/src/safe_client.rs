// Copyright (c) 2021, Facebook, Inc. and its affiliates
// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::authority_client::{AuthorityAPI, BatchInfoResponseItemStream};
use futures::StreamExt;
use sui_types::batch::{AuthorityBatch, SignedBatch, TxSequenceNumber, UpdateItem};
use sui_types::crypto::AuthorityPublicKeyBytes;
use sui_types::messages_checkpoint::{
    AuthenticatedCheckpoint, AuthorityCheckpointInfo, CheckpointContents, CheckpointRequest,
    CheckpointRequestType, CheckpointResponse, CheckpointSequenceNumber,
};
use sui_types::{base_types::*, committee::*, fp_ensure};
use sui_types::{
    error::{SuiError, SuiResult},
    messages::*,
};
use tracing::info;

#[derive(Clone)]
pub struct SafeClient<C> {
    authority_client: C,
    committee: Committee,
    address: AuthorityPublicKeyBytes,
}

impl<C> SafeClient<C> {
    pub fn new(
        authority_client: C,
        committee: Committee,
        address: AuthorityPublicKeyBytes,
    ) -> Self {
        Self {
            authority_client,
            committee,
            address,
        }
    }

    pub fn authority_client(&self) -> &C {
        &self.authority_client
    }

    #[cfg(test)]
    pub fn authority_client_mut(&mut self) -> &mut C {
        &mut self.authority_client
    }

    // Here we centralize all checks for transaction info responses
    fn check_transaction_response(
        &self,
        digest: &TransactionDigest,
        effects_digest: Option<&TransactionEffectsDigest>,
        response: &TransactionInfoResponse,
    ) -> SuiResult {
        if let Some(signed_transaction) = &response.signed_transaction {
            // Check the transaction signature
            signed_transaction.verify(&self.committee)?;
            // Check it has the right signer
            fp_ensure!(
                signed_transaction.auth_sign_info.authority == self.address,
                SuiError::ByzantineAuthoritySuspicion {
                    authority: self.address,
                    reason: "Unexpected validator address in the signed tx signature".to_string()
                }
            );
            // Check it's the right transaction
            fp_ensure!(
                signed_transaction.digest() == digest,
                SuiError::ByzantineAuthoritySuspicion {
                    authority: self.address,
                    reason: "Unexpected digest in the signed tx".to_string()
                }
            );
        }

        if let Some(certificate) = &response.certified_transaction {
            // Check signatures and quorum
            certificate.verify(&self.committee)?;
            // Check it's the right transaction
            fp_ensure!(
                certificate.digest() == digest,
                SuiError::ByzantineAuthoritySuspicion {
                    authority: self.address,
                    reason: "Unexpected digest in the certified tx".to_string()
                }
            );
        }

        if let Some(signed_effects) = &response.signed_effects {
            // Check signature
            signed_effects.verify(&self.committee)?;
            // Check it has the right signer
            fp_ensure!(
                signed_effects.auth_signature.authority == self.address,
                SuiError::ByzantineAuthoritySuspicion {
                    authority: self.address,
                    reason: "Unexpected validator address in the signed effects signature"
                        .to_string()
                }
            );
            // Checks it concerns the right tx
            fp_ensure!(
                signed_effects.effects.transaction_digest == *digest,
                SuiError::ByzantineAuthoritySuspicion {
                    authority: self.address,
                    reason: "Unexpected tx digest in the signed effects".to_string()
                }
            );
            // check that the effects digest is correct.
            if let Some(effects_digest) = effects_digest {
                fp_ensure!(
                    signed_effects.digest() == effects_digest,
                    SuiError::ByzantineAuthoritySuspicion {
                        authority: self.address,
                        reason: "Effects digest does not match with expected digest".to_string()
                    }
                );
            }
        }

        Ok(())
    }

    fn check_object_response(
        &self,
        request: &ObjectInfoRequest,
        response: &ObjectInfoResponse,
    ) -> SuiResult {
        // If we get a certificate make sure it is a valid certificate
        if let Some(certificate) = &response.parent_certificate {
            certificate.verify(&self.committee)?;
        }

        // Check the right object ID and version is returned
        if let Some((object_id, version, _)) = &response.requested_object_reference {
            fp_ensure!(
                object_id == &request.object_id,
                SuiError::ByzantineAuthoritySuspicion {
                    authority: self.address,
                    reason: "Object ID mismatch".to_string()
                }
            );
            if let ObjectInfoRequestKind::PastObjectInfo(requested_version) = &request.request_kind
            {
                fp_ensure!(
                    version == requested_version,
                    SuiError::ByzantineAuthoritySuspicion {
                        authority: self.address,
                        reason: "Object version mismatch".to_string()
                    }
                );
            }
        }

        if let Some(object_and_lock) = &response.object_and_lock {
            // We should only be returning the object and lock data if requesting the latest object info.
            fp_ensure!(
                matches!(
                    request.request_kind,
                    ObjectInfoRequestKind::LatestObjectInfo(_)
                ),
                SuiError::ByzantineAuthoritySuspicion {
                    authority: self.address,
                    reason:
                        "Object and lock data returned when request kind is not LatestObjectInfo"
                            .to_string()
                }
            );

            match response.requested_object_reference {
                Some(obj_ref) => {
                    // Since we are requesting the latest version, we should validate that if the object's
                    // reference actually match with the one from the responded object reference.
                    fp_ensure!(
                        object_and_lock.object.compute_object_reference() == obj_ref,
                        SuiError::ByzantineAuthoritySuspicion {
                            authority: self.address,
                            reason: "Requested object reference mismatch with returned object"
                                .to_string()
                        }
                    );
                }
                None => {
                    // Since we are returning the object for the latest version,
                    // we must also have the requested object reference in the response.
                    // Otherwise the authority has inconsistent data.
                    return Err(SuiError::ByzantineAuthoritySuspicion {
                        authority: self.address,
                        reason: "Object returned without the object reference in response"
                            .to_string(),
                    });
                }
            };

            if let Some(signed_transaction) = &object_and_lock.lock {
                signed_transaction.verify(&self.committee)?;
                // Check it has the right signer
                fp_ensure!(
                    signed_transaction.auth_sign_info.authority == self.address,
                    SuiError::ByzantineAuthoritySuspicion {
                        authority: self.address,
                        reason: "Unexpected validator address in the signed tx signature"
                            .to_string()
                    }
                );
            }
        }

        Ok(())
    }

    fn check_update_item_batch_response(
        &self,
        _request: BatchInfoRequest,
        signed_batch: &SignedBatch,
        transactions_and_last_batch: &Option<(
            Vec<(TxSequenceNumber, ExecutionDigests)>,
            AuthorityBatch,
        )>,
    ) -> SuiResult {
        // check the signature of the batch
        signed_batch.verify(&self.committee)?;

        // ensure transactions enclosed match requested range

        // TODO: check that the batch is within bounds given that the
        //      bounds may now not be known by the requester.
        //
        // if let Some(start) = &request.start {
        //    fp_ensure!(
        //        signed_batch.batch.initial_sequence_number >= *start
        //            && signed_batch.batch.next_sequence_number
        //                <= (*start + request.length + signed_batch.batch.size),
        //        SuiError::ByzantineAuthoritySuspicion {
        //            authority: self.address
        //        }
        //    );
        // }

        // If we have seen a previous batch, use it to make sure the next batch
        // is constructed correctly:

        if let Some((transactions, prev_batch)) = transactions_and_last_batch {
            fp_ensure!(
                !transactions.is_empty(),
                SuiError::GenericAuthorityError {
                    error: "Safe Client: Batches must have some contents.".to_string()
                }
            );
            let reconstructed_batch = AuthorityBatch::make_next(prev_batch, transactions)?;

            fp_ensure!(
                reconstructed_batch == signed_batch.batch,
                SuiError::ByzantineAuthoritySuspicion {
                    authority: self.address,
                    reason: "Inconsistent batch".to_string()
                }
            );
        }

        Ok(())
    }

    /// This function is used by the higher level authority logic to report an
    /// error that could be due to this authority.
    pub fn report_client_error(&self, error: &SuiError) {
        info!(?error, authority =? self.address, "Client error");
    }
}

impl<C> SafeClient<C>
where
    C: AuthorityAPI + Send + Sync + Clone + 'static,
{
    /// Uses the follower API and augments each digest received with a full transactions info structure.
    pub async fn handle_batch_stream_request_to_transaction_info(
        &self,
        request: BatchInfoRequest,
    ) -> Result<
        impl futures::Stream<Item = Result<(u64, TransactionInfoResponse), SuiError>> + '_,
        SuiError,
    > {
        let new_stream = self
            .handle_batch_stream(request)
            .await
            .map_err(|err| SuiError::GenericAuthorityError {
                error: format!("Stream error: {:?}", err),
            })?
            .filter_map(|item| {
                let _client = self.clone();
                async move {
                    match &item {
                        Ok(BatchInfoResponseItem(UpdateItem::Batch(_signed_batch))) => None,
                        Ok(BatchInfoResponseItem(UpdateItem::Transaction((seq, digest)))) => {
                            // Download the full transaction info
                            let transaction_info_request =
                                TransactionInfoRequest::from(digest.transaction);
                            let res = _client
                                .handle_transaction_info_request(transaction_info_request)
                                .await
                                .map(|v| (*seq, v));
                            Some(res)
                        }
                        Err(err) => Some(Err(err.clone())),
                    }
                }
            });

        Ok(new_stream)
    }

    /// Initiate a new transfer to a Sui or Primary account.
    pub async fn handle_transaction(
        &self,
        transaction: Transaction,
    ) -> Result<TransactionInfoResponse, SuiError> {
        let digest = *transaction.digest();
        let transaction_info = self
            .authority_client
            .handle_transaction(transaction)
            .await?;
        if let Err(err) = self.check_transaction_response(&digest, None, &transaction_info) {
            self.report_client_error(&err);
            return Err(err);
        }
        Ok(transaction_info)
    }

    fn verify_certificate_response(
        &self,
        digest: &TransactionDigest,
        response: &TransactionInfoResponse,
    ) -> SuiResult {
        fp_ensure!(
            response.signed_effects.is_some(),
            SuiError::ByzantineAuthoritySuspicion {
                authority: self.address,
                reason: "An Ok response from handle_certificate must contain signed effects"
                    .to_string()
            }
        );
        self.check_transaction_response(digest, None, response)
    }

    /// Execute a certificate.
    pub async fn handle_certificate(
        &self,
        certificate: CertifiedTransaction,
    ) -> Result<TransactionInfoResponse, SuiError> {
        let digest = *certificate.digest();
        let transaction_info = self
            .authority_client
            .handle_certificate(certificate)
            .await?;

        if let Err(err) = self.verify_certificate_response(&digest, &transaction_info) {
            self.report_client_error(&err);
            return Err(err);
        }
        Ok(transaction_info)
    }

    pub async fn handle_account_info_request(
        &self,
        request: AccountInfoRequest,
    ) -> Result<AccountInfoResponse, SuiError> {
        self.authority_client
            .handle_account_info_request(request)
            .await
    }

    pub async fn handle_object_info_request(
        &self,
        request: ObjectInfoRequest,
    ) -> Result<ObjectInfoResponse, SuiError> {
        let response = self
            .authority_client
            .handle_object_info_request(request.clone())
            .await?;
        if let Err(err) = self.check_object_response(&request, &response) {
            self.report_client_error(&err);
            return Err(err);
        }
        Ok(response)
    }

    /// Handle Transaction information requests for this account.
    pub async fn handle_transaction_info_request(
        &self,
        request: TransactionInfoRequest,
    ) -> Result<TransactionInfoResponse, SuiError> {
        let digest = request.transaction_digest;
        let transaction_info = self
            .authority_client
            .handle_transaction_info_request(request)
            .await?;

        if let Err(err) = self.check_transaction_response(&digest, None, &transaction_info) {
            self.report_client_error(&err);
            return Err(err);
        }
        Ok(transaction_info)
    }

    /// Handle Transaction + Effects information requests for this account.
    pub async fn handle_transaction_and_effects_info_request(
        &self,
        digests: &ExecutionDigests,
    ) -> Result<TransactionInfoResponse, SuiError> {
        let transaction_info = self
            .authority_client
            .handle_transaction_info_request(digests.transaction.into())
            .await?;

        if let Err(err) = self.check_transaction_response(
            &digests.transaction,
            Some(&digests.effects),
            &transaction_info,
        ) {
            self.report_client_error(&err);
            return Err(err);
        }
        Ok(transaction_info)
    }

    fn verify_checkpoint_sequence(
        &self,
        expected_seq: Option<CheckpointSequenceNumber>,
        checkpoint: &Option<AuthenticatedCheckpoint>,
    ) -> SuiResult {
        let observed_seq = checkpoint.as_ref().map(|c| c.summary().sequence_number);

        if let (Some(e), Some(o)) = (expected_seq, observed_seq) {
            fp_ensure!(
                e == o,
                SuiError::from("Expected checkpoint number doesn't match with returned")
            );
        }
        Ok(())
    }

    fn verify_contents_exist<T>(
        &self,
        request_content: bool,
        checkpoint: &Option<T>,
        contents: &Option<CheckpointContents>,
    ) -> SuiResult {
        match (request_content, checkpoint, contents) {
            // If content is requested, checkpoint is not None, but we are not getting any content,
            // it's an error.
            // If content is not requested, or checkpoint is None, yet we are still getting content,
            // it's an error.
            (true, Some(_), None) | (false, _, Some(_)) | (_, None, Some(_)) => Err(
                SuiError::from("Checkpoint contents inconsistent with request"),
            ),
            _ => Ok(()),
        }
    }

    fn verify_checkpoint_response(
        &self,
        request: &CheckpointRequest,
        response: &CheckpointResponse,
    ) -> SuiResult {
        // Verify signatures
        response.verify(&self.committee)?;

        // Verify response data was correct for request
        match &request.request_type {
            CheckpointRequestType::AuthenticatedCheckpoint(seq) => {
                if let AuthorityCheckpointInfo::AuthenticatedCheckpoint(checkpoint) = &response.info
                {
                    // Checks that the sequence number is correct.
                    self.verify_checkpoint_sequence(*seq, checkpoint)?;
                    self.verify_contents_exist(request.detail, checkpoint, &response.detail)
                } else {
                    Err(SuiError::from(
                        "Invalid AuthorityCheckpointInfo type in the response",
                    ))
                }
            }
            CheckpointRequestType::CheckpointProposal => {
                if let AuthorityCheckpointInfo::CheckpointProposal {
                    proposal,
                    prev_cert: _,
                } = &response.info
                {
                    self.verify_contents_exist(request.detail, proposal, &response.detail)
                } else {
                    Err(SuiError::from(
                        "Invalid AuthorityCheckpointInfo type in the response",
                    ))
                }
            }
        }
    }

    pub async fn handle_checkpoint(
        &self,
        request: CheckpointRequest,
    ) -> Result<CheckpointResponse, SuiError> {
        let resp = self
            .authority_client
            .handle_checkpoint(request.clone())
            .await?;
        self.verify_checkpoint_response(&request, &resp)
            .map_err(|err| {
                self.report_client_error(&err);
                err
            })?;
        Ok(resp)
    }

    /// Handle Batch information requests for this authority.
    pub async fn handle_batch_stream(
        &self,
        request: BatchInfoRequest,
    ) -> Result<BatchInfoResponseItemStream, SuiError> {
        let batch_info_items = self
            .authority_client
            .handle_batch_stream(request.clone())
            .await?;

        let client = self.clone();
        let address = self.address;
        let count: u64 = 0;
        let stream = Box::pin(batch_info_items.scan(
            (None, count),
            move |(txs_and_last_batch, count), batch_info_item| {
                let req_clone = request.clone();
                let client = client.clone();

                // We check if we have exceeded the batch boundary for this request.
                // This is to protect against server DoS
                if *count > 10 * request.length {
                    // If we exceed it return None to end stream
                    return futures::future::ready(None);
                }
                let result = match &batch_info_item {
                    Ok(BatchInfoResponseItem(UpdateItem::Batch(signed_batch))) => {
                        if let Err(err) = client.check_update_item_batch_response(
                            req_clone,
                            signed_batch,
                            txs_and_last_batch,
                        ) {
                            client.report_client_error(&err);
                            Some(Err(err))
                        } else {
                            // Insert a fresh vector for the new batch of transactions
                            let _ =
                                txs_and_last_batch.insert((Vec::new(), signed_batch.batch.clone()));
                            Some(batch_info_item)
                        }
                    }
                    Ok(BatchInfoResponseItem(UpdateItem::Transaction((seq, digest)))) => {
                        // A stream always starts with a batch, so the previous should have initialized it.
                        // And here we insert the tuple into the batch.
                        match txs_and_last_batch {
                            None => {
                                let err = SuiError::ByzantineAuthoritySuspicion {
                                    authority: address,
                                    reason: "Stream does not start with a batch".to_string(),
                                };
                                client.report_client_error(&err);
                                Some(Err(err))
                            }
                            Some(txs) => {
                                txs.0.push((*seq, *digest));

                                *count += 1;
                                Some(batch_info_item)
                            }
                        }
                    }
                    Err(e) => Some(Err(e.clone())),
                };

                futures::future::ready(result)
            },
        ));
        Ok(Box::pin(stream))
    }
}
