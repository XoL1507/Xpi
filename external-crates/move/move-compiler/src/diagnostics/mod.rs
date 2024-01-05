// Copyright (c) The Diem Core Contributors
// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

pub mod codes;

use crate::{
    command_line::COLOR_MODE_ENV_VAR,
    diagnostics::codes::{Category, DiagnosticCode, DiagnosticInfo, Severity, WarningFilter},
    shared::ast_debug::AstDebug,
};
use codespan_reporting::{
    self as csr,
    files::SimpleFiles,
    term::{
        emit,
        termcolor::{Buffer, ColorChoice, StandardStream, WriteColor},
        Config,
    },
};
use move_command_line_common::{env::read_env_var, files::FileHash};
use move_ir_types::location::*;
use move_symbol_pool::Symbol;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    iter::FromIterator,
    ops::Range,
};

use self::codes::{UnusedItem, WARNING_FILTER_ATTR};

//**************************************************************************************************
// Types
//**************************************************************************************************

pub type FileId = usize;
pub type FileName = Symbol;

pub type FilesSourceText = HashMap<FileHash, (FileName, String)>;
type FileMapping = HashMap<FileHash, FileId>;

#[derive(PartialEq, Eq, Clone, Debug, Hash)]
#[must_use]
pub struct Diagnostic {
    info: DiagnosticInfo,
    primary_label: (Loc, String),
    secondary_labels: Vec<(Loc, String)>,
    notes: Vec<String>,
}

#[derive(PartialEq, Eq, Hash, Clone, Debug, Default)]
pub struct Diagnostics {
    diagnostics: Vec<Diagnostic>,
    severity_count: BTreeMap<Severity, usize>,
}

#[derive(PartialEq, Eq, Clone, Debug)]
/// Used to filter out diagnostics, specifically used for warning suppression
pub enum WarningFilters {
    /// Remove all warnings
    All,
    /// Remove all diags of this category
    Specified {
        category: BTreeSet</* category */ u8>,
        /// Remove specific diags
        codes: BTreeSet<(/* category */ u8, /* code */ u8)>,
    },
    /// No filter
    Empty,
}

//**************************************************************************************************
// Reporting
//**************************************************************************************************

pub fn report_diagnostics(files: &FilesSourceText, diags: Diagnostics) -> ! {
    let should_exit = true;
    report_diagnostics_impl(files, diags, should_exit);
    std::process::exit(1)
}

pub fn report_warnings(files: &FilesSourceText, warnings: Diagnostics) {
    if warnings.is_empty() {
        return;
    }
    debug_assert!(warnings.max_severity().unwrap() == Severity::Warning);
    report_diagnostics_impl(files, warnings, false)
}

fn report_diagnostics_impl(files: &FilesSourceText, diags: Diagnostics, should_exit: bool) {
    let color_choice = match read_env_var(COLOR_MODE_ENV_VAR).as_str() {
        "NONE" => ColorChoice::Never,
        "ANSI" => ColorChoice::AlwaysAnsi,
        "ALWAYS" => ColorChoice::Always,
        _ => ColorChoice::Auto,
    };
    let mut writer = StandardStream::stderr(color_choice);
    output_diagnostics(&mut writer, files, diags);
    if should_exit {
        std::process::exit(1);
    }
}

pub fn unwrap_or_report_diagnostics<T>(files: &FilesSourceText, res: Result<T, Diagnostics>) -> T {
    match res {
        Ok(t) => t,
        Err(diags) => {
            assert!(!diags.is_empty());
            report_diagnostics(files, diags)
        }
    }
}

pub fn report_diagnostics_to_buffer(files: &FilesSourceText, diags: Diagnostics) -> Vec<u8> {
    let mut writer = Buffer::no_color();
    output_diagnostics(&mut writer, files, diags);
    writer.into_inner()
}

pub fn report_diagnostics_to_color_buffer(files: &FilesSourceText, diags: Diagnostics) -> Vec<u8> {
    let mut writer = Buffer::ansi();
    output_diagnostics(&mut writer, files, diags);
    writer.into_inner()
}

fn output_diagnostics<W: WriteColor>(
    writer: &mut W,
    sources: &FilesSourceText,
    diags: Diagnostics,
) {
    let mut files = SimpleFiles::new();
    let mut file_mapping = HashMap::new();
    for (fhash, (fname, source)) in sources {
        let id = files.add(*fname, source.as_str());
        file_mapping.insert(*fhash, id);
    }
    render_diagnostics(writer, &files, &file_mapping, diags);
}

fn render_diagnostics(
    writer: &mut dyn WriteColor,
    files: &SimpleFiles<Symbol, &str>,
    file_mapping: &FileMapping,
    mut diags: Diagnostics,
) {
    diags.diagnostics.sort_by(|e1, e2| {
        let loc1: &Loc = &e1.primary_label.0;
        let loc2: &Loc = &e2.primary_label.0;
        loc1.cmp(loc2)
    });
    let mut seen: HashSet<Diagnostic> = HashSet::new();
    for diag in diags.diagnostics {
        if seen.contains(&diag) {
            continue;
        }
        seen.insert(diag.clone());
        let rendered = render_diagnostic(file_mapping, diag);
        emit(writer, &Config::default(), files, &rendered).unwrap()
    }
}

fn convert_loc(file_mapping: &FileMapping, loc: Loc) -> (FileId, Range<usize>) {
    let fname = loc.file_hash();
    let id = *file_mapping.get(&fname).unwrap();
    let range = loc.usize_range();
    (id, range)
}

fn render_diagnostic(
    file_mapping: &FileMapping,
    diag: Diagnostic,
) -> csr::diagnostic::Diagnostic<FileId> {
    use csr::diagnostic::{Label, LabelStyle};
    let mk_lbl = |style: LabelStyle, msg: (Loc, String)| -> Label<FileId> {
        let (id, range) = convert_loc(file_mapping, msg.0);
        csr::diagnostic::Label::new(style, id, range).with_message(msg.1)
    };
    let Diagnostic {
        info,
        primary_label,
        secondary_labels,
        notes,
    } = diag;
    let mut diag = csr::diagnostic::Diagnostic::new(info.severity().into_codespan_severity());
    let (code, message) = info.render();
    diag = diag.with_code(code);
    diag = diag.with_message(message.to_string());
    diag = diag.with_labels(vec![mk_lbl(LabelStyle::Primary, primary_label)]);
    diag = diag.with_labels(
        secondary_labels
            .into_iter()
            .map(|msg| mk_lbl(LabelStyle::Secondary, msg))
            .collect(),
    );
    diag = diag.with_notes(notes);
    diag
}

//**************************************************************************************************
// impls
//**************************************************************************************************

impl Diagnostics {
    pub fn new() -> Self {
        Self {
            diagnostics: vec![],
            severity_count: BTreeMap::new(),
        }
    }

    pub fn max_severity(&self) -> Option<Severity> {
        debug_assert!(self.severity_count.values().all(|count| *count > 0));
        self.severity_count
            .iter()
            .max_by_key(|(sev, _count)| **sev)
            .map(|(sev, _count)| *sev)
    }

    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    pub fn len(&self) -> usize {
        self.diagnostics.len()
    }

    pub fn add(&mut self, diag: Diagnostic) {
        *self.severity_count.entry(diag.info.severity()).or_insert(0) += 1;
        self.diagnostics.push(diag)
    }

    pub fn add_opt(&mut self, diag_opt: Option<Diagnostic>) {
        if let Some(diag) = diag_opt {
            self.add(diag)
        }
    }

    pub fn extend(&mut self, other: Self) {
        let Self {
            diagnostics,
            severity_count,
        } = other;
        for (sev, count) in severity_count {
            *self.severity_count.entry(sev).or_insert(0) += count;
        }
        self.diagnostics.extend(diagnostics)
    }

    pub fn into_vec(self) -> Vec<Diagnostic> {
        self.diagnostics
    }

    pub fn into_codespan_format(
        self,
    ) -> Vec<(
        codespan_reporting::diagnostic::Severity,
        &'static str,
        (Loc, String),
        Vec<(Loc, String)>,
        Vec<String>,
    )> {
        let mut v = vec![];
        for diag in self.into_vec() {
            let Diagnostic {
                info,
                primary_label,
                secondary_labels,
                notes,
            } = diag;
            let csr_diag = (
                info.severity().into_codespan_severity(),
                info.message(),
                primary_label,
                secondary_labels,
                notes,
            );
            v.push(csr_diag)
        }
        v
    }
}

impl Diagnostic {
    pub fn new(
        code: impl Into<DiagnosticInfo>,
        (loc, label): (Loc, impl ToString),
        secondary_labels: impl IntoIterator<Item = (Loc, impl ToString)>,
        notes: impl IntoIterator<Item = impl ToString>,
    ) -> Self {
        Diagnostic {
            info: code.into(),
            primary_label: (loc, label.to_string()),
            secondary_labels: secondary_labels
                .into_iter()
                .map(|(loc, msg)| (loc, msg.to_string()))
                .collect(),
            notes: notes.into_iter().map(|msg| msg.to_string()).collect(),
        }
    }

    pub fn set_code(mut self, code: impl Into<DiagnosticInfo>) -> Self {
        self.info = code.into();
        self
    }

    #[allow(unused)]
    pub fn add_secondary_labels(
        &mut self,
        additional_labels: impl IntoIterator<Item = (Loc, impl ToString)>,
    ) {
        self.secondary_labels.extend(
            additional_labels
                .into_iter()
                .map(|(loc, msg)| (loc, msg.to_string())),
        )
    }

    pub fn add_secondary_label(&mut self, (loc, msg): (Loc, impl ToString)) {
        self.secondary_labels.push((loc, msg.to_string()))
    }

    pub fn extra_labels_len(&self) -> usize {
        self.secondary_labels.len() + self.notes.len()
    }

    #[allow(unused)]
    pub fn add_notes(&mut self, additional_notes: impl IntoIterator<Item = impl ToString>) {
        self.notes
            .extend(additional_notes.into_iter().map(|msg| msg.to_string()))
    }

    pub fn add_note(&mut self, msg: impl ToString) {
        self.notes.push(msg.to_string())
    }

    pub fn info(&self) -> &DiagnosticInfo {
        &self.info
    }
}

#[macro_export]
macro_rules! diag {
    ($code: expr, $primary: expr $(,)?) => {{
        #[allow(unused)]
        use $crate::diagnostics::codes::*;
        $crate::diagnostics::Diagnostic::new(
            $code,
            $primary,
            std::iter::empty::<(move_ir_types::location::Loc, String)>(),
            std::iter::empty::<String>(),
        )
    }};
    ($code: expr, $primary: expr, $($secondary: expr),+ $(,)?) => {{
        #[allow(unused)]
        use $crate::diagnostics::codes::*;
        $crate::diagnostics::Diagnostic::new(
            $code,
            $primary,
            vec![$($secondary, )*],
            std::iter::empty::<String>(),
        )
    }};
}

impl WarningFilters {
    pub fn new() -> Self {
        Self::Empty
    }

    pub fn is_filtered(&self, diag: &Diagnostic) -> bool {
        self.is_filtered_by_info(&diag.info)
    }

    fn is_filtered_by_info(&self, info: &DiagnosticInfo) -> bool {
        match self {
            WarningFilters::All => info.severity() == Severity::Warning,
            WarningFilters::Specified { category, codes } => {
                info.severity() == Severity::Warning
                    && (category.contains(&info.category())
                        || codes.contains(&(info.category(), info.code())))
            }
            WarningFilters::Empty => false,
        }
    }

    pub fn union(&mut self, other: &WarningFilters) {
        match (self, other) {
            // if self is empty, just take the other filter
            (s @ Self::Empty, _) => *s = other.clone(),
            // if other is empty, or self is ALL, no change to the filter
            (_, Self::Empty) => (),
            (Self::All, _) => (),
            // if other is all, self is now all
            (s, Self::All) => *s = Self::All,
            // category and code level union
            (
                Self::Specified { category, codes },
                Self::Specified {
                    category: other_category,
                    codes: other_codes,
                },
            ) => {
                category.extend(other_category);
                // remove any codes covered by the category level filter
                codes.extend(
                    other_codes
                        .iter()
                        .filter(|(codes_cat, _)| !category.contains(codes_cat)),
                );
            }
        }
    }

    pub fn add(&mut self, filter: WarningFilter) {
        match self {
            WarningFilters::All => (),
            WarningFilters::Empty => {
                *self = WarningFilters::Specified {
                    category: BTreeSet::new(),
                    codes: BTreeSet::new(),
                };
                return self.add(filter);
            }
            WarningFilters::Specified { category, codes } => match filter {
                WarningFilter::All => *self = WarningFilters::All,
                WarningFilter::Category(cat) => {
                    let cat = cat as u8;
                    category.insert(cat);
                    // remove any codes now covered by this category
                    codes.retain(|(codes_cat, _)| codes_cat != &cat);
                }
                WarningFilter::Code(cat, code) => {
                    let cat = cat as u8;
                    // no need to add the filter if already covered by the category
                    if !category.contains(&cat) {
                        codes.insert((cat, code));
                    }
                }
            },
        }
    }

    pub fn unused_function_warnings_filter() -> Self {
        let unused_fn_info = UnusedItem::Function.into_info();
        let filtered_codes =
            BTreeSet::from([(unused_fn_info.category() as u8, unused_fn_info.code())]);
        WarningFilters::Specified {
            category: BTreeSet::new(),
            codes: filtered_codes,
        }
    }
}

//**************************************************************************************************
// traits
//**************************************************************************************************

impl FromIterator<Diagnostic> for Diagnostics {
    fn from_iter<I: IntoIterator<Item = Diagnostic>>(iter: I) -> Self {
        let diagnostics = iter.into_iter().collect::<Vec<_>>();
        Self::from(diagnostics)
    }
}

impl From<Vec<Diagnostic>> for Diagnostics {
    fn from(diagnostics: Vec<Diagnostic>) -> Self {
        let mut severity_count = BTreeMap::new();
        for diag in &diagnostics {
            *severity_count.entry(diag.info.severity()).or_insert(0) += 1;
        }
        Self {
            diagnostics,
            severity_count,
        }
    }
}

impl From<Option<Diagnostic>> for Diagnostics {
    fn from(diagnostic_opt: Option<Diagnostic>) -> Self {
        Diagnostics::from(diagnostic_opt.map_or_else(Vec::new, |diag| vec![diag]))
    }
}

impl AstDebug for WarningFilters {
    fn ast_debug(&self, w: &mut crate::shared::ast_debug::AstWriter) {
        match self {
            WarningFilters::All => w.write(&format!(
                "#[{}({})]",
                WARNING_FILTER_ATTR,
                WarningFilter::All.to_str().unwrap(),
            )),
            WarningFilters::Specified { category, codes } => {
                w.write(&format!("#[{}(", WARNING_FILTER_ATTR,));
                let items = category
                    .iter()
                    .copied()
                    .map(|cat| WarningFilter::Category(Category::try_from(cat).unwrap()))
                    .chain(codes.iter().copied().map(|(cat, code)| {
                        WarningFilter::Code(Category::try_from(cat).unwrap(), code)
                    }));
                w.list(items, ",", |w, filter| {
                    w.write(filter.to_str().unwrap());
                    false
                });
                w.write(")]")
            }
            WarningFilters::Empty => (),
        }
    }
}

impl<C: DiagnosticCode> From<C> for DiagnosticInfo {
    fn from(value: C) -> Self {
        value.into_info()
    }
}
