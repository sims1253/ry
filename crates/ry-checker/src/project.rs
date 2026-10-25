//! Project-level checking over one `FnTable` shared by every file.
//!
//! 1. Pass 1 collects function definitions from every file.
//! 2. Pass 2 runs the fixpoint over the shared table, so cross-file
//!    return types converge.
//! 3. Pass 3 emits each file's diagnostics against the refined table.

use crate::declaration_check::DeclarationSet;
use crate::definitions::Callee;
use crate::{
    CallerVisibleSignature, Checker, DeclarationFinding, Diagnostic, FnTable, FxSet, ReturnSlots,
};
use rayon::prelude::*;
use ry_core::SourceFile;
use ry_typeshed::Typeshed;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

struct FileEmission {
    index: usize,
    path: String,
    diagnostics: Vec<Diagnostic>,
    declaration_findings: Vec<DeclarationFinding>,
    scopes: Vec<crate::ScopeRecord>,
    references: crate::ReferenceFacts,
    read_fns: HashSet<String>,
}

/// A multi-file R project. Functions defined in any file are visible
/// to all other files. If two files define a top-level function with the
/// same name, the file added later wins, as with R's `source()` order.
#[derive(Default)]
pub struct Project {
    /// Shared function table, collected by pass 1 and refined by pass 2.
    fn_table: FnTable,
    /// Shared inferred return types, refined by pass 2's fixpoint loop.
    return_slots: ReturnSlots,
    files: Vec<(String, Arc<SourceFile>)>,
    /// Per-file diagnostics from the last check, reused for files outside
    /// the dirty set.
    diagnostics: Vec<(String, Vec<Diagnostic>)>,
    /// Opt-in authored records and their latest per-file findings. The
    /// original records remain available to the adapter/export layer.
    declarations: Arc<DeclarationSet>,
    declaration_findings: Vec<(String, Vec<DeclarationFinding>)>,
    /// Declared packages unioned with every file's `library`/`require`
    /// attachments. The dplyr NSE gating reads this project-wide view.
    loaded: std::collections::HashSet<String>,
    /// Packages explicitly configured by the caller. Kept separate from
    /// `loaded`, which also contains packages discovered in source files,
    /// so removing a `library()` call during an incremental edit removes
    /// that package from the next project-wide union.
    declared_loaded: HashSet<String>,
    /// Per-file bare-name search paths, kept apart from `loaded`.
    bare_loaded: HashMap<String, HashSet<String>>,
    /// Names bound by project metadata such as `NAMESPACE` imports:
    /// `importFrom(shiny, tags)` binds `tags` in every package source file.
    /// They resolve to opaque values.
    external_bindings: HashMap<String, HashSet<String>>,
    package_facts: HashMap<String, ry_workspace::PackageFacts>,
    imported_from: HashMap<String, HashMap<String, String>>,
    external_s3_methods: HashMap<String, HashSet<(String, String)>>,
    load_bindings: HashMap<String, HashMap<usize, HashSet<String>>>,
    user_stubs: Arc<BTreeMap<String, Typeshed>>,
    /// Paths open in an editor. A box module at such a path that this
    /// project does not check is opaque: its disk copy may be stale.
    open_buffer_paths: HashSet<String>,
    /// Pass-1 output per source path.
    collected_files: HashMap<String, CollectedFile>,
    /// Top-level bindings per file, pooled for emission.
    file_known_vars: HashMap<String, HashSet<String>>,
    /// Paths whose content changed since the last emit.
    dirty_paths: HashSet<String>,
    /// Function names from invalidated pass-1 entries, so the reverse call
    /// graph still reaches callers of a removed or renamed function.
    invalidated_fns: HashSet<String>,
    /// The `loaded` set from the previous emit. A change re-emits every file.
    prev_loaded: Option<HashSet<String>>,
    /// Whether `refine_and_emit` has completed at least once.
    has_prev_emit: bool,
    /// Function names each file calls (from `call_sites`).
    file_called_fns: HashMap<String, HashSet<String>>,
    /// Actual callable reads during emission include callbacks and aliases
    /// absent from syntactic call sites. Names survive return-slot renumbering.
    file_read_fns: HashMap<String, HashSet<String>>,
    /// Actual per-function reads from completed refinement rounds. These only
    /// limit refinement. File emission keeps its conservative dependencies.
    refinement_dependencies: HashMap<String, HashSet<String>>,
    /// Alias-based attachments are absent from collection and can affect any body.
    refinement_discovered_attachments: bool,
    #[cfg(test)]
    last_refinement_counts: HashMap<String, usize>,
    /// Refined return types from the previous run, keyed by function name.
    /// They seed the next fixpoint.
    prev_fn_returns: HashMap<String, ry_core::RType>,
    /// Previous caller-visible parameter signatures, keyed by function name.
    /// Return slots alone are insufficient: argument names, order, required
    /// status, evaluation semantics, and parameter types all affect callers.
    prev_fn_signatures: HashMap<String, CallerVisibleSignature>,
    /// Project-wide table facts from the previous emit.
    prev_tables: TableSnapshot,
    /// When true, pass-3 emitters snapshot each file's lexical scopes.
    /// Off by default. See [`Checker::enable_scope_capture`].
    capture_scopes: bool,
    capture_references: bool,
    reference_facts: Vec<(String, crate::ReferenceFacts)>,
    /// Scope records from the last emission, one entry per re-emitted
    /// file. Only a cold `check()` emits every file.
    scope_records: Vec<(String, Vec<crate::ScopeRecord>)>,
    /// How many files the last `refine_and_emit` emitted rather than
    /// served from cache. Tests assert on it.
    #[doc(hidden)]
    pub emit_count: usize,
}

#[derive(Clone)]
pub(crate) struct CollectedFile {
    pub(crate) fn_table: FnTable,
    pub(crate) return_slots: ReturnSlots,
    pub(crate) loaded: HashSet<String>,
}

/// A package namespace's share of the project-wide binding pools, built from
/// its `R/` files only. R builds a namespace from `R/`, while tests and
/// scripts run in their own environments, so neither their top-level
/// assignments nor their `<<-` writes bind names for package code.
#[derive(Default, PartialEq)]
struct NamespacePool {
    known_vars: HashSet<String>,
    superassigned: HashSet<String>,
}

/// A namespace pool shared with pass-3 emitters: (known_vars, superassigned).
type SharedNamespacePool = (Arc<HashSet<String>>, Arc<HashSet<String>>);

/// Project-wide table facts that can change any file's diagnostics without
/// changing a function name or return slot, so a change re-emits every file.
#[derive(Default, PartialEq)]
struct TableSnapshot {
    /// Non-function bindings across files (RY010).
    known_vars: HashSet<String>,
    /// Per package, the bindings of its `R/` files only.
    namespaces: HashMap<String, NamespacePool>,
    /// Another file's registration, or literal binding, changes whether a
    /// method gets its dispatch bindings.
    registered_s3_methods: FxSet<String>,
    unrecorded_global_literals: FxSet<String>,
    /// A write-only edit can change which captured declaration is provable.
    capture_rebounds: FxSet<(String, usize, usize)>,
    /// Another file's top-level effects can remove a deferred call's
    /// related location.
    unproven_top_level: bool,
    /// Callable bindings without return slots affect call resolution.
    callable_vars: HashSet<String>,
    /// Generic metadata changes which method a call selects
    /// (`signature = c("y", "x")`).
    s4_inventory: crate::definitions::S4Inventory,
    escaped_operator_names: bool,
    escaped_slot_names: bool,
    may_install_bindings: bool,
    /// Another file's `<<-` widens bindings read in function bodies.
    superassigned: HashSet<String>,
}

impl TableSnapshot {
    fn of(table: &FnTable, namespaces: HashMap<String, NamespacePool>) -> Self {
        Self {
            known_vars: table.known_vars.clone(),
            namespaces,
            registered_s3_methods: table.registered_s3_methods.clone(),
            unrecorded_global_literals: table.unrecorded_global_literals.clone(),
            capture_rebounds: table.rebound_after_capture.clone(),
            unproven_top_level: table.unproven_top_level,
            callable_vars: table.callable_vars.clone(),
            s4_inventory: table.functions.s4().clone(),
            escaped_operator_names: table.has_escaped_operator_names,
            escaped_slot_names: table.has_escaped_slot_names,
            may_install_bindings: table.may_install_bindings,
            superassigned: table.superassigned.clone(),
        }
    }
}

/// Replace `current` with `new` when they differ, returning whether the
/// replacement happened.
fn set_if_changed<T: PartialEq>(current: &mut T, new: T) -> bool {
    if *current == new {
        false
    } else {
        *current = new;
        true
    }
}

impl Project {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the project's complete adopted/candidate record set. An
    /// annotation-only edit changes this input even when R ASTs and inferred
    /// returns are identical, so every file is re-emitted on the next warm
    /// check. Reinstalling identical records is a no-op.
    pub fn set_declaration_records(
        &mut self,
        records: Vec<ry_core::declarations::DeclarationRecord>,
    ) {
        if self.declarations.records() == records.as_slice() {
            return;
        }
        self.declarations = Arc::new(DeclarationSet::new(records));
        self.declaration_findings.clear();
        self.dirty_paths
            .extend(self.files.iter().map(|(path, _)| path.clone()));
    }

    pub fn declaration_records(&self) -> &[ry_core::declarations::DeclarationRecord] {
        self.declarations.records()
    }

    pub fn declaration_findings(&self) -> &[(String, Vec<DeclarationFinding>)] {
        &self.declaration_findings
    }

    /// Add a parsed file to the project. A file added later shadows
    /// same-named top-level definitions of earlier files.
    ///
    /// Two files may share a path, since a lossy display path can name
    /// distinct files: [`check`](Self::check) merges both, while the
    /// incremental caches key files by path.
    pub fn add_file(&mut self, path: String, file: SourceFile) {
        self.add_file_arc(path, Arc::new(file));
    }

    /// [`add_file`](Self::add_file) for a file already in an `Arc`.
    pub fn add_file_arc(&mut self, path: String, file: Arc<SourceFile>) {
        self.dirty_paths.insert(path.clone());
        self.files.push((path, file));
    }

    /// Replace an existing parsed file while preserving project order, or
    /// append it when the path is new. Only that file's pass-1 cache entry is
    /// invalidated. `check_incremental` reuses every other file's collection.
    pub fn update_file(&mut self, path: String, file: Arc<SourceFile>) {
        if let Some(previous) = self.collected_files.remove(&path) {
            self.invalidated_fns
                .extend(previous.fn_table.functions.names().cloned());
        }
        self.file_known_vars.remove(&path);
        self.dirty_paths.insert(path.clone());
        if let Some((_, existing)) = self
            .files
            .iter_mut()
            .find(|(existing_path, _)| existing_path == &path)
        {
            *existing = file;
        } else {
            self.files.push((path, file));
        }
        // A module can supply exports to unchanged importers, which the
        // function-name dependency graph does not describe.
        if self.has_box_imports() {
            self.mark_all_dirty();
        }
    }

    /// Reinstall the project's files in the order given by `order`, so
    /// same-name shadowing follows `order` instead of insertion history.
    /// Paths missing from `order` sort after the listed ones. The caches
    /// key on name and path and cannot see a reorder, so a reorder that
    /// moves a file invalidates every name defined in more than one file,
    /// with its defining files. A reorder that moves nothing is a no-op.
    pub fn reorder_files(&mut self, order: &[String]) {
        let rank: HashMap<&str, usize> = order
            .iter()
            .enumerate()
            .map(|(index, path)| (path.as_str(), index))
            .collect();
        // A stable sort cannot change an order that already satisfies
        // the key, so one adjacent-inversion scan decides whether the
        // reorder is a no-op before the in-place sort borrows the list.
        let key = |path: &str| rank.get(path).copied().unwrap_or(usize::MAX);
        let moved = self
            .files
            .windows(2)
            .any(|pair| key(pair[0].0.as_str()) > key(pair[1].0.as_str()));
        self.files.sort_by_key(|(path, _)| key(path.as_str()));
        if !moved || self.collected_files.is_empty() {
            return;
        }
        // Only a name defined in more than one file can change its winner.
        // A name defined twice in one file resolves inside that file's
        // collection.
        let mut definers: HashMap<&str, usize> = HashMap::new();
        for collected in self.collected_files.values() {
            for name in collected.fn_table.functions.names() {
                *definers.entry(name).or_insert(0) += 1;
            }
        }
        let shadowed: HashSet<&str> = definers
            .into_iter()
            .filter_map(|(name, count)| (count > 1).then_some(name))
            .collect();
        if shadowed.is_empty() {
            return;
        }
        for (path, collected) in &self.collected_files {
            if collected
                .fn_table
                .functions
                .names()
                .any(|name| shadowed.contains(name.as_str()))
            {
                self.dirty_paths.insert(path.clone());
            }
        }
        for name in shadowed {
            self.invalidated_fns.insert(name.to_string());
        }
    }

    /// Remove a file and its cached pass-1 collection from the project.
    pub fn remove_file(&mut self, path: &str) {
        self.files.retain(|(existing, _)| existing != path);
        if let Some(previous) = self.collected_files.remove(path) {
            self.invalidated_fns
                .extend(previous.fn_table.functions.names().cloned());
        }
        self.file_known_vars.remove(path);
        self.file_read_fns.remove(path);
        // Removing a file changes the shared table and pooled known_vars,
        // so every remaining file re-emits.
        self.mark_all_dirty();
    }

    /// Declare the project's loaded packages (from `ry.toml`'s `packages`
    /// key). Reinstalling the set already in `declared_loaded` is a no-op.
    pub fn set_loaded(&mut self, loaded: std::collections::HashSet<String>) {
        if self.declared_loaded == loaded {
            return;
        }
        self.declared_loaded = loaded.clone();
        self.loaded = loaded;
        self.mark_all_dirty();
    }

    pub fn set_bare_loaded(&mut self, loaded: HashMap<String, HashSet<String>>) {
        if set_if_changed(&mut self.bare_loaded, loaded) {
            self.mark_all_dirty();
        }
    }

    fn has_box_imports(&self) -> bool {
        self.files
            .iter()
            .any(|(_, file)| crate::box_imports::has_box_use(file))
    }

    /// Record the paths open in an editor (`open_buffer_paths`).
    pub fn set_open_buffer_paths(&mut self, paths: HashSet<String>) {
        if self.open_buffer_paths != paths {
            self.open_buffer_paths = paths;
            if self.has_box_imports() {
                self.mark_all_dirty();
            }
        }
    }

    /// Mark every file dirty so the next incremental check re-emits all.
    fn mark_all_dirty(&mut self) {
        let paths: Vec<String> = self.files.iter().map(|(p, _)| p.clone()).collect();
        for p in paths {
            self.dirty_paths.insert(p);
        }
    }

    /// Opt this project into snapshotting every file's lexical scopes
    /// during the next check. The records replace those of the previous
    /// emission and are read back with
    /// [`take_scope_records`](Self::take_scope_records).
    pub fn enable_scope_capture(&mut self) {
        self.capture_scopes = true;
        self.scope_records.clear();
    }

    /// Take the scope records captured by the most recent check. Empty
    /// unless [`enable_scope_capture`](Self::enable_scope_capture) was
    /// called before it.
    pub fn take_scope_records(&mut self) -> Vec<(String, Vec<crate::ScopeRecord>)> {
        std::mem::take(&mut self.scope_records)
    }

    /// Capture reference facts for every file on each subsequent check.
    /// This bypasses the diagnostic cache so the snapshot is complete.
    pub fn enable_reference_capture(&mut self) {
        self.capture_references = true;
        self.reference_facts.clear();
    }

    /// Take the reference facts from the most recent check.
    pub fn take_reference_facts(&mut self) -> Vec<(String, crate::ReferenceFacts)> {
        std::mem::take(&mut self.reference_facts)
    }

    /// Install runtime package stubs. User packages, including `base`,
    /// replace same-named embedded packages wholesale. Installing the same
    /// `Arc` again is a no-op. A different set clears every cache.
    pub fn set_user_stubs(&mut self, stubs: Arc<BTreeMap<String, Typeshed>>) {
        if Arc::ptr_eq(&self.user_stubs, &stubs) {
            return;
        }
        self.collected_files.clear();
        self.user_stubs = stubs;
        self.mark_all_dirty();
    }

    /// Declare per-file names provided by project metadata, such as
    /// `NAMESPACE`'s `importFrom()` directives. Per-file scoping prevents an
    /// import in one checked package from leaking into an unrelated package.
    pub fn set_external_bindings(&mut self, bindings: HashMap<String, HashSet<String>>) {
        if set_if_changed(&mut self.external_bindings, bindings) {
            self.mark_all_dirty();
        }
    }

    pub fn set_package_facts(&mut self, facts: HashMap<String, ry_workspace::PackageFacts>) {
        if set_if_changed(&mut self.package_facts, facts) {
            self.collected_files.clear();
            self.mark_all_dirty();
        }
    }

    pub fn set_imported_from(&mut self, imports: HashMap<String, HashMap<String, String>>) {
        if set_if_changed(&mut self.imported_from, imports) {
            self.mark_all_dirty();
        }
    }

    pub fn set_external_s3_methods(&mut self, methods: HashMap<String, HashSet<(String, String)>>) {
        if set_if_changed(&mut self.external_s3_methods, methods) {
            self.mark_all_dirty();
        }
    }

    pub fn set_load_bindings(
        &mut self,
        bindings: HashMap<String, HashMap<usize, HashSet<String>>>,
    ) {
        if set_if_changed(&mut self.load_bindings, bindings) {
            self.mark_all_dirty();
        }
    }

    /// Run the three-pass check from scratch and return each file's
    /// diagnostics in input order. For edits, use
    /// [`update_file`](Self::update_file) and
    /// [`check_incremental`](Self::check_incremental) instead.
    pub fn check(&mut self) -> Vec<(String, Vec<Diagnostic>)> {
        // Discard every collection and refinement cache, so the shared path
        // below collects all files, refines everything, and emits every file.
        self.collected_files.clear();
        self.file_known_vars.clear();
        self.dirty_paths = self.files.iter().map(|(p, _)| p.clone()).collect();
        self.prev_loaded = None;
        self.has_prev_emit = false;
        self.prev_fn_returns.clear();
        self.file_read_fns.clear();
        self.refinement_dependencies.clear();
        self.prev_fn_signatures.clear();
        self.prev_tables = TableSnapshot::default();
        self.invalidated_fns.clear();
        // Merge every entry rather than one cached collection per path: a
        // lossy display path can name two distinct files.
        let collected = self.collect_uncached();
        let merged = self.merge_collected(collected.iter().map(|(_, collected)| collected));
        self.collected_files.extend(collected);
        self.install_and_emit(merged)
    }

    /// Check after one or more `update_file` calls, reusing pass-1
    /// collection for every unchanged file.
    pub fn check_incremental(&mut self) -> Vec<(String, Vec<Diagnostic>)> {
        let collected = self.collect_uncached();
        self.collected_files.extend(collected);
        let merged = self.merge_collected(self.cached_in_order());
        self.install_and_emit(merged)
    }

    /// Pass 1: one collection walk per uncached file harvests its functions
    /// and its `library`/`require` attachments. Files are independent, so
    /// the walks fan out on rayon's pool. Results keep file order.
    fn collect_uncached(&mut self) -> Vec<(String, CollectedFile)> {
        let collected: Vec<_> = self
            .files
            .par_iter()
            .filter(|(path, _)| !self.collected_files.contains_key(path))
            .map(|(path, file)| {
                let mut collector = Checker::new(path);
                collector.set_user_stubs(Arc::clone(&self.user_stubs));
                collector
                    .set_package_facts(self.package_facts.get(path).cloned().unwrap_or_default());
                let loaded = collector.collect_file_fns(file);
                let (fn_table, return_slots) = collector.into_tables();
                let collected = CollectedFile {
                    fn_table,
                    return_slots,
                    loaded,
                };
                (path.clone(), collected)
            })
            .collect();
        for (path, collected) in &collected {
            self.file_known_vars
                .insert(path.clone(), collected.fn_table.known_vars.clone());
        }
        collected
    }

    fn cached_in_order(&self) -> impl Iterator<Item = &CollectedFile> {
        self.files
            .iter()
            .map(|(path, _)| &self.collected_files[path])
    }

    /// Merge collections in file order, so a later file's same-named
    /// function replaces the earlier one's. Attachments in any file are
    /// unioned with the declared packages.
    fn merge_collected<'a>(
        &self,
        collections: impl Iterator<Item = &'a CollectedFile>,
    ) -> (FnTable, ReturnSlots, HashSet<String>) {
        let mut fn_table = FnTable::default();
        let mut return_slots = ReturnSlots::default();
        let mut loaded = self.declared_loaded.clone();
        for collected in collections {
            loaded.extend(collected.loaded.iter().cloned());
            fn_table.append_collected(
                &collected.fn_table,
                &mut return_slots,
                &collected.return_slots,
            );
        }
        fn_table.known_vars = self.pooled_known_vars();
        (fn_table, return_slots, loaded)
    }

    fn install_and_emit(
        &mut self,
        (fn_table, return_slots, loaded): (FnTable, ReturnSlots, HashSet<String>),
    ) -> Vec<(String, Vec<Diagnostic>)> {
        for (path, _) in &self.files {
            self.file_called_fns.insert(
                path.clone(),
                self.collected_files[path]
                    .fn_table
                    .call_sites
                    .keys()
                    .cloned()
                    .collect(),
            );
        }
        self.loaded = loaded;
        self.fn_table = fn_table;
        self.return_slots = return_slots;
        self.refine_and_emit()
    }

    /// A pass-2 checker over `fn_table` that records refinement reads.
    fn refiner(
        &self,
        fn_table: FnTable,
        return_slots: ReturnSlots,
        box_modules: &Arc<crate::box_imports::BoxModules>,
    ) -> Checker {
        let mut refiner = Checker::with_tables("__project_pass2__", fn_table, return_slots);
        refiner.set_loaded(self.loaded.clone());
        refiner.set_user_stubs(Arc::clone(&self.user_stubs));
        refiner.set_box_modules(Arc::clone(box_modules));
        // Pass 2 has no per-file context. Without the registrations, a
        // package's own method for a stub generic (`group_by.tbl_lazy`)
        // would be read as the stub's default-receiver return.
        refiner.set_external_s3_methods(
            self.external_s3_methods
                .values()
                .flatten()
                .cloned()
                .collect(),
        );
        refiner.refinement_dependencies = Some(HashMap::new());
        refiner
    }

    /// The function names that need fixpoint refinement, or `None` for
    /// all of them. The set holds dirty-file definitions and their observed
    /// callers, including forwarding and S3 metadata dependencies.
    fn compute_fixpoint_scope(&self) -> Option<HashSet<String>> {
        if !self.has_prev_emit || self.refinement_discovered_attachments {
            return None;
        }
        // Package signatures affect return types.
        if self
            .prev_loaded
            .as_ref()
            .is_some_and(|prev| prev != &self.loaded)
        {
            return None;
        }
        // A new callable can resolve an unknown callback or alias, and no
        // observed dependency records the earlier lookup miss.
        let prev = &self.prev_tables;
        if self.callable_names_changed()
            || prev.callable_vars != self.fn_table.callable_vars
            || prev.s4_inventory != *self.fn_table.functions.s4()
            || prev.escaped_operator_names != self.fn_table.has_escaped_operator_names
            || prev.escaped_slot_names != self.fn_table.has_escaped_slot_names
            || prev.may_install_bindings != self.fn_table.may_install_bindings
            // A registration changes another file's method frame.
            || prev.registered_s3_methods != self.fn_table.registered_s3_methods
        {
            return None;
        }
        if self.dirty_paths.is_empty() {
            return Some(HashSet::new());
        }
        // Functions defined in dirty files, plus definitions removed or
        // renamed by those edits, seed the affected set.
        let mut affected = self.invalidated_fns.clone();
        // Any file's write can withdraw a registered method's dispatch
        // bindings (#348).
        affected.extend(
            self.fn_table
                .functions
                .names()
                .filter(|name| {
                    self.fn_table
                        .registered_s3_methods
                        .contains(crate::infer::semantic_argument_name(name))
                })
                .cloned(),
        );
        for dirty_path in &self.dirty_paths {
            if let Some(collected) = self.collected_files.get(dirty_path) {
                affected.extend(collected.fn_table.functions.names().cloned());
            }
        }
        Some(self.with_refinement_callers(affected))
    }

    fn with_refinement_callers(&self, mut affected: HashSet<String>) -> HashSet<String> {
        let methods = crate::fixpoint::s3_evaluation_methods(&self.fn_table);
        let mut callers: HashMap<&str, HashSet<&str>> = HashMap::new();
        for (caller, dependencies) in &self.refinement_dependencies {
            for dependency in dependencies {
                callers.entry(dependency).or_default().insert(caller);
            }
        }
        // Evaluation metadata is propagated outside the body read recorder.
        // Include these edges even before any quoting/injection is present.
        for call in &self.fn_table.forwarded_calls {
            if self
                .fn_table
                .functions
                .select(Callee::parse(&call.stub_callee))
                .is_some()
                && self.fn_table.functions.binds(&call.caller)
            {
                callers
                    .entry(&call.callee)
                    .or_default()
                    .insert(&call.caller);
            }
        }
        for (generic, methods) in &methods {
            for method in methods {
                callers.entry(method).or_default().insert(generic);
            }
        }
        let mut pending: Vec<_> = affected.iter().cloned().collect();
        while let Some(callee) = pending.pop() {
            for caller in callers.get(callee.as_str()).into_iter().flatten() {
                if affected.insert(caller.to_string()) {
                    pending.push(caller.to_string());
                }
            }
        }
        affected
    }

    fn callable_names_changed(&self) -> bool {
        self.fn_table.functions.len() != self.prev_fn_returns.len()
            || self
                .fn_table
                .functions
                .names()
                .any(|name| !self.prev_fn_returns.contains_key(name))
    }

    /// Whether a file reads or calls any affected function.
    fn file_depends_on(&self, path: &str, affected: &HashSet<String>) -> bool {
        self.file_called_fns
            .get(path)
            .into_iter()
            .chain(self.file_read_fns.get(path))
            .flatten()
            .any(|callee| affected.contains(callee))
    }

    /// Expand a set of changed callees through the cached reverse call graph.
    /// Syntactic calls and observed callable reads are collected per file,
    /// so every function in a dependent file is a conservative caller.
    /// Repeating to a fixpoint reaches callers in other files transitively.
    fn with_transitive_callers(&self, mut affected: HashSet<String>) -> HashSet<String> {
        let mut changed = true;
        while changed {
            changed = false;
            for (path, collected) in &self.collected_files {
                if !self.file_depends_on(path, &affected) {
                    continue;
                }
                for caller in collected.fn_table.functions.names() {
                    changed |= affected.insert(caller.clone());
                }
            }
        }
        affected
    }

    fn refine_and_emit(&mut self) -> Vec<(String, Vec<Diagnostic>)> {
        // Box overlays are keyed by physical identity. A native path keeps
        // a lossy display path from colliding with a distinct Unicode module;
        // unsaved buffers use their logical path.
        let mut box_sources = HashMap::new();
        if self.has_box_imports() {
            for path in &self.open_buffer_paths {
                if let Some(identity) = crate::box_imports::path_identity(Path::new(path)) {
                    box_sources.insert(identity, None);
                }
            }
            for (path, file) in &self.files {
                let path = file.native_path.as_deref().unwrap_or(Path::new(path));
                if let Some(identity) = crate::box_imports::path_identity(path) {
                    box_sources.insert(identity, Some(Arc::clone(file)));
                }
            }
        }
        // Pass 2 and pass 3 share one module inventory per run.
        let box_modules = Arc::new(crate::box_imports::BoxModules::new(box_sources));
        // Pass 2. Compute the scope before moving the tables into the
        // refiner, because `fn_table` records S3 generic-to-method edges.
        let mut fixpoint_scope = self.compute_fixpoint_scope();
        let fn_table = std::mem::take(&mut self.fn_table);
        let return_slots = std::mem::take(&mut self.return_slots);
        let mut refiner = self.refiner(fn_table, return_slots, &box_modules);

        // Only a scoped run reuses the previous refined returns. A full run
        // starts from fresh collection, like a cold check, because old
        // metadata can belong to a replaced or shadowed definition and
        // recursive returns can preserve an old seed.
        if let Some(ref scope) = fixpoint_scope {
            refiner.seed_return_types(&self.prev_fn_returns, scope);
            refiner.seed_caller_visible_signatures(&self.prev_fn_signatures, scope);
        }
        refiner.run_fixpoint_scoped(fixpoint_scope.as_ref());
        if fixpoint_scope.is_some() && *refiner.loaded != self.loaded {
            // Alias-based library/require calls can change every bare lookup.
            // Rebuild from collection: widening the seeded scope could retain
            // stale recursive returns or argument evaluation metadata.
            #[cfg(test)]
            let attempted_counts = std::mem::take(&mut refiner.refinement_counts);
            let (table, slots, _) = self.merge_collected(self.cached_in_order());
            refiner = self.refiner(table, slots, &box_modules);
            refiner.run_fixpoint_scoped(None);
            #[cfg(test)]
            for (name, count) in attempted_counts {
                *refiner.refinement_counts.entry(name).or_default() += count;
            }
            fixpoint_scope = None;
        }
        self.refinement_discovered_attachments = *refiner.loaded != self.loaded;
        if fixpoint_scope.is_none() {
            self.refinement_dependencies.clear();
        }
        self.refinement_dependencies
            .extend(refiner.refinement_dependencies.take().unwrap());
        self.refinement_dependencies
            .retain(|name, _| refiner.fn_table.functions.binds(name));
        #[cfg(test)]
        {
            self.last_refinement_counts = std::mem::take(&mut refiner.refinement_counts);
        }
        let (fn_table, return_slots) = refiner.into_tables();
        self.fn_table = fn_table;
        self.return_slots = return_slots;

        // Re-emit only files whose diagnostics can have changed.
        let loaded_changed = self
            .prev_loaded
            .as_ref()
            .is_none_or(|prev| prev != &self.loaded);

        // Compute functions whose return type or complete caller-visible
        // parameter signature changed. Snapshots are keyed by name because
        // slot indices shift when functions are inserted or removed.
        let directly_changed_fns: HashSet<String> = self
            .fn_table
            .functions
            .live()
            .filter_map(|(name, function)| {
                let current_return = self.return_slots.0.get(function.return_slot);
                let return_changed = self
                    .prev_fn_returns
                    .get(name)
                    .is_none_or(|previous| current_return != Some(previous));
                let current_signature = function.caller_visible_signature();
                let signature_changed = self
                    .prev_fn_signatures
                    .get(name)
                    .is_none_or(|previous| previous != &current_signature);
                (return_changed || signature_changed).then(|| name.clone())
            })
            .collect();
        let changed_fns = self.with_transitive_callers(
            directly_changed_fns
                .into_iter()
                .chain(self.invalidated_fns.iter().cloned())
                .collect(),
        );

        // S3/S4 methods: conservatively re-emit all when any callable state changed.
        let changed_s3: HashSet<usize> = if changed_fns.is_empty() {
            HashSet::new()
        } else {
            self.fn_table
                .s3_methods
                .values()
                .copied()
                .chain(self.fn_table.functions.s4_method_slots())
                .collect()
        };

        let tables = TableSnapshot::of(&self.fn_table, self.namespaces());
        let must_emit: HashSet<&str> = if !self.has_prev_emit
            || loaded_changed
            || self.prev_tables != tables
            || self.callable_names_changed()
        {
            self.files.iter().map(|(p, _)| p.as_str()).collect()
        } else {
            let mut dirty: HashSet<&str> = self.dirty_paths.iter().map(|s| s.as_str()).collect();
            for (path, _) in &self.files {
                // A file re-emits when it reads a changed function. S3
                // dispatch is dynamic, so a changed S3/S4 method slot
                // conservatively re-emits every file.
                if !changed_s3.is_empty() || self.file_depends_on(path, &changed_fns) {
                    dirty.insert(path.as_str());
                }
            }
            dirty
        };

        // Pass 3. Each emitter shares the refined tables through an `Arc`.
        // Pass 3 never mutates them.
        let fn_table = Arc::new(std::mem::take(&mut self.fn_table));
        let package_known_vars = Arc::new(fn_table.known_vars.clone());
        let namespaces: HashMap<&str, SharedNamespacePool> = tables
            .namespaces
            .iter()
            .map(|(package, pool)| {
                let shared = (
                    Arc::new(pool.known_vars.clone()),
                    Arc::new(pool.superassigned.clone()),
                );
                (package.as_str(), shared)
            })
            .collect();
        let return_slots = Arc::new(std::mem::take(&mut self.return_slots));
        let loaded = Arc::new(std::mem::take(&mut self.loaded));
        let user_stubs = Arc::clone(&self.user_stubs);
        let declarations = Arc::clone(&self.declarations);

        let emit_indices: Vec<usize> = self
            .files
            .iter()
            .enumerate()
            .filter(|(_, (path, _))| self.capture_references || must_emit.contains(path.as_str()))
            .map(|(i, _)| i)
            .collect();
        self.emit_count = emit_indices.len();
        let capture_scopes = self.capture_scopes;
        let capture_references = self.capture_references;

        let mut names_by_slot: HashMap<usize, Vec<&String>> = HashMap::new();
        for (name, function) in fn_table.functions.live() {
            names_by_slot
                .entry(function.return_slot)
                .or_default()
                .push(name);
        }
        let per_file: Vec<FileEmission> = emit_indices
            .par_iter()
            .map(|&i| {
                let (path, file) = &self.files[i];
                let mut emitter = Checker::with_shared_tables(
                    path,
                    Arc::clone(&fn_table),
                    Arc::clone(&return_slots),
                );
                emitter.set_shared_declarations(Arc::clone(&declarations));
                let namespace = self
                    .source_package(path)
                    .and_then(|package| namespaces.get(package));
                emitter.set_shared_known_vars(Arc::clone(
                    namespace.map_or(&package_known_vars, |(known_vars, _)| known_vars),
                ));
                emitter.set_namespace_superassigned(
                    namespace.map(|(_, superassigned)| Arc::clone(superassigned)),
                );
                emitter.set_shared_loaded(Arc::clone(&loaded));
                emitter.set_bare_loaded(
                    self.bare_loaded
                        .get(path)
                        .cloned()
                        // Direct Project users only have the declared set;
                        // CLI installs precise per-file paths above.
                        .unwrap_or_else(|| loaded.as_ref().clone()),
                );
                emitter.set_user_stubs(Arc::clone(&user_stubs));
                emitter.set_box_modules(Arc::clone(&box_modules));
                emitter.set_external_bindings(
                    self.external_bindings
                        .get(path)
                        .cloned()
                        .unwrap_or_default(),
                );
                emitter
                    .set_package_facts(self.package_facts.get(path).cloned().unwrap_or_default());
                emitter
                    .set_imported_from(self.imported_from.get(path).cloned().unwrap_or_default());
                emitter.set_external_s3_methods(
                    self.external_s3_methods
                        .get(path)
                        .cloned()
                        .unwrap_or_default(),
                );
                emitter
                    .set_load_bindings(self.load_bindings.get(path).cloned().unwrap_or_default());
                if capture_scopes {
                    emitter.enable_scope_capture();
                }
                if capture_references {
                    emitter.enable_reference_capture();
                }
                *emitter.refinement_reads.get_mut() = Some(Vec::new());
                emitter.emit_diagnostics(file);
                let read_slots: HashSet<_> = emitter
                    .refinement_reads
                    .get_mut()
                    .take()
                    .unwrap()
                    .into_iter()
                    .collect();
                let read_fns = read_slots
                    .iter()
                    .filter_map(|slot| names_by_slot.get(slot))
                    .flatten()
                    .map(|name| (*name).clone())
                    .collect();
                let records = emitter.take_scope_records();
                let references = emitter.take_reference_facts();
                FileEmission {
                    index: i,
                    path: path.clone(),
                    diagnostics: emitter.take_diagnostics(),
                    declaration_findings: emitter.take_declaration_findings(),
                    scopes: records,
                    references,
                    read_fns,
                }
            })
            .collect();

        // Every emitter is dropped, so `unwrap_or_clone` does not clone.
        self.fn_table = Arc::unwrap_or_clone(fn_table);
        self.return_slots = Arc::unwrap_or_clone(return_slots);
        self.loaded = Arc::unwrap_or_clone(loaded);

        let mut per_file = per_file;
        for emission in &mut per_file {
            self.file_read_fns.insert(
                emission.path.clone(),
                std::mem::take(&mut emission.read_fns),
            );
        }
        if capture_scopes {
            self.scope_records = per_file
                .iter_mut()
                .map(|emission| (emission.path.clone(), std::mem::take(&mut emission.scopes)))
                .collect();
        }

        if capture_references {
            self.reference_facts = per_file
                .iter_mut()
                .map(|emission| {
                    (
                        emission.path.clone(),
                        std::mem::take(&mut emission.references),
                    )
                })
                .collect();
        }

        // Files outside the emit set reuse their previous results, looked
        // up by path: repeated linear scans would make an unchanged project
        // check quadratic in its file count. A path added twice shares one
        // cache entry, so its second copy falls back to empty results.
        let mut cached_diagnostics: HashMap<_, _> =
            std::mem::take(&mut self.diagnostics).into_iter().collect();
        let mut cached_findings: HashMap<_, _> = std::mem::take(&mut self.declaration_findings)
            .into_iter()
            .collect();
        // `per_file` follows `emit_indices`, which is in file order.
        let mut emitted = per_file.into_iter().peekable();
        let (result, declaration_findings): (Vec<_>, Vec<_>) = self
            .files
            .iter()
            .enumerate()
            .map(|(index, (path, _))| {
                let (diagnostics, findings) =
                    match emitted.next_if(|emission| emission.index == index) {
                        Some(emission) => (emission.diagnostics, emission.declaration_findings),
                        None => (
                            cached_diagnostics.remove(path).unwrap_or_default(),
                            cached_findings.remove(path).unwrap_or_default(),
                        ),
                    };
                ((path.clone(), diagnostics), (path.clone(), findings))
            })
            .unzip();
        self.declaration_findings = declaration_findings;

        self.prev_loaded = Some(self.loaded.clone());
        self.has_prev_emit = true;
        self.prev_tables = tables;
        self.prev_fn_returns = self
            .fn_table
            .functions
            .live()
            .map(|(name, uf)| (name.clone(), self.return_slots.get(uf.return_slot)))
            .collect();
        self.prev_fn_signatures = self
            .fn_table
            .functions
            .live()
            .map(|(name, function)| (name.clone(), function.caller_visible_signature()))
            .collect();
        self.dirty_paths.clear();
        self.invalidated_fns.clear();

        self.diagnostics = result.clone();
        result
    }

    fn pooled_known_vars(&self) -> HashSet<String> {
        self.file_known_vars
            .values()
            .flat_map(|known_vars| known_vars.iter().cloned())
            .collect()
    }

    fn source_package(&self, path: &str) -> Option<&str> {
        self.package_facts.get(path)?.source_package.as_deref()
    }

    /// Each package's [`NamespacePool`].
    fn namespaces(&self) -> HashMap<String, NamespacePool> {
        let mut pools: HashMap<String, NamespacePool> = HashMap::new();
        for (path, _) in &self.files {
            if let Some(package) = self.source_package(path) {
                let pool = pools.entry(package.to_string()).or_default();
                let collected = &self.collected_files[path].fn_table;
                pool.known_vars.extend(collected.known_vars.iter().cloned());
                pool.superassigned
                    .extend(collected.superassigned.iter().cloned());
            }
        }
        pools
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::parse_file;

    fn assert_matches_cold(project: &mut Project) -> Vec<(String, Vec<Diagnostic>)> {
        let actual = project.check_incremental();
        let mut cold = Project::new();
        for (path, file) in &project.files {
            cold.add_file(path.clone(), (**file).clone());
        }
        let expected = cold.check();
        assert_eq!(project.prev_fn_returns, cold.prev_fn_returns);
        assert_eq!(project.prev_fn_signatures, cold.prev_fn_signatures);
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
        actual
    }

    #[test]
    fn only_an_own_package_qualified_call_is_default_evidence() {
        for (package, flagged) in [("mypkg", true), ("xfun", false)] {
            let mut project = Project::new();
            project.set_package_facts(
                ["R/a.R", "R/b.R"]
                    .map(|path| {
                        let facts = ry_workspace::PackageFacts {
                            source_package: Some("mypkg".into()),
                            ..Default::default()
                        };
                        (path.to_string(), facts)
                    })
                    .into(),
            );
            project.add_file(
                "R/a.R".into(),
                parse_file(
                    "R/a.R",
                    "normalize_path <- function(path = 'none') path + 1L",
                ),
            );
            let caller = format!("use <- function() {package}::normalize_path()");
            project.add_file("R/b.R".into(), parse_file("R/b.R", &caller));
            let diagnostics = project.check();
            let a = &diagnostics
                .iter()
                .find(|(path, _)| path == "R/a.R")
                .unwrap()
                .1;
            assert_eq!(a.iter().any(|d| d.code == "RY040"), flagged, "{a:?}");
        }
    }

    #[test]
    fn reordering_a_generic_signature_reselects_methods_in_other_files() {
        let generic = |order: &str| {
            let source = format!(
                "setGeneric(\"g\", function(x, y) standardGeneric(\"g\"), signature = c({order}))\n"
            );
            parse_file("generic.R", &source)
        };
        let mut project = Project::new();
        project.add_file("generic.R".into(), generic("\"x\", \"y\""));
        project.add_file(
            "method.R".into(),
            parse_file(
                "method.R",
                "setClass(\"A\", representation(v = \"numeric\"))\n\
                 setMethod(\"g\", signature(\"A\", \"character\"), function(x, y) \"chr\")\n\
                 setMethod(\"g\", signature(\"A\", \"numeric\"), function(x, y) 1L)\n\
                 g(new(\"A\"), \"t\") + 1L\n",
            ),
        );
        let flagged = |diagnostics: &[(String, Vec<Diagnostic>)]| {
            diagnostics
                .iter()
                .any(|(path, found)| path == "method.R" && found.iter().any(|d| d.code == "RY040"))
        };
        assert!(flagged(&project.check()));
        // The positional entries now mean `(y = A, x = character)`.
        for (order, expected) in [("\"y\", \"x\"", false), ("\"x\", \"y\"", true)] {
            project.update_file("generic.R".into(), generic(order).into());
            assert_eq!(flagged(&assert_matches_cold(&mut project)), expected);
        }
    }

    #[test]
    fn another_files_s3_registration_gives_dispatch_bindings() {
        let mut project = Project::new();
        project.add_file(
            "method.R".into(),
            parse_file("method.R", "compare <- function(e1, e2) .Generic\n"),
        );
        let registration = "evalqOnLoad(registerS3method(\"<\", \"widget\", compare))\n";
        project.add_file("zzz.R".into(), parse_file("zzz.R", registration));
        project.add_file("other.R".into(), parse_file("other.R", "x <- 1\n"));
        let flagged = |diagnostics: &[(String, Vec<Diagnostic>)]| {
            diagnostics
                .iter()
                .any(|(path, found)| path == "method.R" && found.iter().any(|d| d.code == "RY010"))
        };
        assert!(!flagged(&project.check()));
        // Editing the registration, or rebinding `compare` in a third file,
        // withdraws the bindings; undoing the edit restores them.
        for (path, source, expected) in [
            (
                "zzz.R",
                "evalqOnLoad(if (FALSE) registerS3method(\"<\", \"widget\", compare))\n",
                true,
            ),
            ("zzz.R", registration, false),
            ("other.R", "compare <- 2\n", true),
            ("other.R", "x <- 1\n", false),
            ("other.R", "{\n  compare <- function(e1, e2) e1\n}\n", true),
            ("other.R", "x <- 1\n", false),
            ("other.R", "compare <- function(e1, e2) e1\n", true),
            ("other.R", "x <- 1\n", false),
        ] {
            project.update_file(path.into(), parse_file(path, source).into());
            assert_eq!(
                flagged(&assert_matches_cold(&mut project)),
                expected,
                "{path}: {source}"
            );
        }
    }

    #[test]
    fn namespace_s3_method_edit_gives_dispatch_bindings_incrementally() {
        let source = "format.widget <- function(x, ...) .Generic\n";
        let methods = |registered: bool| {
            let entries = if registered {
                HashSet::from([("format".to_string(), "widget".to_string())])
            } else {
                HashSet::new()
            };
            HashMap::from([("R/a.R".to_string(), entries)])
        };
        let mut project = Project::new();
        project.add_file("R/a.R".into(), parse_file("R/a.R", source));
        project.set_external_s3_methods(methods(false));
        project.check();
        for registered in [true, false] {
            project.set_external_s3_methods(methods(registered));
            let incremental = project.check_incremental();
            let mut cold = Project::new();
            cold.add_file("R/a.R".into(), parse_file("R/a.R", source));
            cold.set_external_s3_methods(methods(registered));
            assert_eq!(format!("{incremental:?}"), format!("{:?}", cold.check()));
            assert_eq!(project.prev_fn_returns, cold.prev_fn_returns);
            let flagged = incremental
                .iter()
                .flat_map(|(_, found)| found)
                .any(|d| d.code == "RY010");
            assert_eq!(flagged, !registered);
        }
    }

    #[test]
    fn function_dependencies_skip_unrelated_callers_and_retain_edges() {
        let mut project = Project::new();
        project.add_file(
            "leaf.R".into(),
            parse_file("leaf.R", "leaf <- function() 1L"),
        );
        let mut callers = "caller <- function() leaf()\n".to_string();
        for i in 0..100 {
            callers.push_str(&format!("stable{i} <- function() {i}L\n"));
        }
        project.add_file("callers.R".into(), parse_file("callers.R", &callers));
        project.add_file(
            "outer.R".into(),
            parse_file("outer.R", "outer <- function() stable0()"),
        );
        project.check();
        for source in ["leaf <- function() 'changed'", "leaf <- function() 1L"] {
            project.update_file("leaf.R".into(), parse_file("leaf.R", source).into());
            assert_matches_cold(&mut project);
            assert!(project.last_refinement_counts.contains_key("leaf"));
            assert!(project.last_refinement_counts.contains_key("caller"));
            assert_eq!(
                project.last_refinement_counts.len(),
                2,
                "{:?}",
                project.last_refinement_counts
            );
            assert!(project.refinement_dependencies["outer"].contains("stable0"));
        }
        project.update_file(
            "callers.R".into(),
            parse_file(
                "callers.R",
                &callers.replace("stable0 <- function() 0L", "stable0 <- function() 'new'"),
            )
            .into(),
        );
        assert_matches_cold(&mut project);
        assert!(project.last_refinement_counts.contains_key("outer"));
    }

    #[test]
    fn reorder_only_change_recomputes_the_winning_definition() {
        // A reorder with no content edit must still flip the winner and
        // match a fresh project in the new order.
        let mut project = Project::new();
        // Insertion order [use.R, z.R, a.R] puts the character `f` last,
        // so it wins the first check and use.R flags RY040.
        project.add_file("use.R".into(), parse_file("use.R", "x <- f() + 1L"));
        project.add_file("z.R".into(), parse_file("z.R", "f <- function() 1L"));
        project.add_file("a.R".into(), parse_file("a.R", "f <- function() \"str\""));
        let first = project.check_incremental();
        let (_, use_diagnostics) = first
            .iter()
            .find(|(path, _)| path == "use.R")
            .expect("use.R checked");
        assert!(
            use_diagnostics.iter().any(|d| d.code == "RY040"),
            "character f wins in insertion order: {use_diagnostics:?}"
        );

        // In sorted order [a.R, use.R, z.R] the integer `f` wins.
        project.reorder_files(&["a.R".to_string(), "use.R".to_string(), "z.R".to_string()]);
        let second = assert_matches_cold(&mut project);
        let (_, use_diagnostics) = second
            .iter()
            .find(|(path, _)| path == "use.R")
            .expect("use.R checked");
        assert!(
            !use_diagnostics.iter().any(|d| d.code == "RY040"),
            "integer f must win after the reorder: {use_diagnostics:?}"
        );
    }

    #[test]
    fn no_op_reorder_keeps_the_warm_state_steady() {
        // A reorder that moves no file keeps the warm result identical
        // and matching a fresh project.
        let mut project = Project::new();
        project.add_file("a.R".into(), parse_file("a.R", "f <- function() \"str\""));
        project.add_file("use.R".into(), parse_file("use.R", "x <- f() + 1L"));
        project.add_file("z.R".into(), parse_file("z.R", "f <- function() 1L"));
        project.reorder_files(&["a.R".to_string(), "use.R".to_string(), "z.R".to_string()]);
        let before = assert_matches_cold(&mut project);
        project.reorder_files(&["a.R".to_string(), "use.R".to_string(), "z.R".to_string()]);
        let after = assert_matches_cold(&mut project);
        assert_eq!(format!("{before:?}"), format!("{after:?}"));
    }

    #[test]
    fn function_dependencies_cover_aliases_callbacks_and_metadata() {
        for (before, after, caller) in [
            (
                "leaf <- function() 1L",
                "leaf <- function() 'x'",
                "caller <- function() { alias <- leaf; alias() }",
            ),
            (
                "leaf <- function(x) 1L",
                "leaf <- function(x) 'x'",
                "caller <- function() lapply(1L, leaf)",
            ),
            (
                "leaf <- function(x) x",
                "leaf <- function(x) substitute(x)",
                "middle <- function(x) leaf(x)\ncaller <- function(x) middle(x)",
            ),
            (
                "generic.foo <- function(x, y) y",
                "generic.foo <- function(x, y) substitute(y)",
                "generic <- function(x, y) UseMethod('generic')\ncaller <- function(x, y) generic(x, y)",
            ),
            (
                "leaf <- function() 1L",
                "leaf <- function() other()",
                "other <- function() leaf()\ncaller <- function() other()",
            ),
            (
                "leaf <- function() 1L",
                "leaf <- function() 1L\nmissing <- function(x) 'x'",
                "caller <- function() lapply(1L, missing)",
            ),
        ] {
            let mut project = Project::new();
            project.add_file("leaf.R".into(), parse_file("leaf.R", before));
            project.add_file("caller.R".into(), parse_file("caller.R", caller));
            project.check();
            for source in [after, before, after] {
                project.update_file("leaf.R".into(), parse_file("leaf.R", source).into());
                assert_matches_cold(&mut project);
            }
        }
    }

    #[test]
    fn function_dependencies_invalidate_changed_callable_bindings() {
        let before = "list <- S7::new_class('Thing')\nz <- function() 1L";
        let after = "list <- NULL\nz <- function() 1L";
        for body in ["list(1, 2)", "list(1, 2); NULL"] {
            let callers = format!(
                "a <- function() {{ list <- 1L; {body} }}\nb <- function() a()\nc <- function() z()"
            );
            let mut project = Project::new();
            project.add_file("s7.R".into(), parse_file("s7.R", before));
            project.add_file("callers.R".into(), parse_file("callers.R", &callers));
            project.add_file(
                "constant.R".into(),
                parse_file(
                    "constant.R",
                    "stable_return <- function() { list <- 1L; list(1, 2); NULL }",
                ),
            );
            project.add_file(
                "top.R".into(),
                parse_file("top.R", "list <- 1L; list(1, 2)"),
            );
            project.check();
            for source in [after, before, after] {
                project.update_file("s7.R".into(), parse_file("s7.R", source).into());
                assert_matches_cold(&mut project);
            }
        }
    }

    #[test]
    fn function_dependencies_include_divergence_probe_helpers() {
        let before = "leaf <- function() 1L";
        let after = "leaf <- function() stop('halt')";
        let source = "middle <- function() { if (FALSE) leaf() else stop('halt') }\ncaller <- function(x) { if (!is.numeric(x)) middle(); x }";
        let mut project = Project::new();
        project.add_file("leaf.R".into(), parse_file("leaf.R", before));
        project.add_file("callers.R".into(), parse_file("callers.R", source));
        project.check();
        let previous = project.prev_fn_returns["caller"].clone();
        assert!(project.refinement_dependencies["middle"].contains("leaf"));
        for source in [after, before] {
            project.update_file("leaf.R".into(), parse_file("leaf.R", source).into());
            assert_matches_cold(&mut project);
            assert!(project.last_refinement_counts.contains_key("caller"));
            if source == after {
                assert_ne!(project.prev_fn_returns["caller"], previous);
            }
        }
    }

    #[test]
    fn scoped_refinement_restarts_after_alias_attachment() {
        let before = "a_attach <- function() NULL";
        let after = "a_attach <- function() { loader <- library; loader(dplyr); NULL }";
        let callers = "z_consumer <- function() a_attach()\nb_unrelated <- function() filter(data.frame(x = 1L), x > 0)";
        let mut warm = Project::new();
        warm.add_file("loader.R".into(), parse_file("loader.R", before));
        warm.add_file("callers.R".into(), parse_file("callers.R", callers));
        warm.check();
        let previous = warm.prev_fn_returns.clone();
        warm.update_file("loader.R".into(), parse_file("loader.R", after).into());
        let warm_diags = warm.check_incremental();
        let mut cold = Project::new();
        cold.add_file("loader.R".into(), parse_file("loader.R", after));
        cold.add_file("callers.R".into(), parse_file("callers.R", callers));
        let cold_diags = cold.check();
        assert_ne!(previous["b_unrelated"], cold.prev_fn_returns["b_unrelated"]);
        assert_eq!(warm.prev_fn_returns, cold.prev_fn_returns);
        assert_eq!(warm.prev_fn_signatures, cold.prev_fn_signatures);
        assert_eq!(format!("{warm_diags:?}"), format!("{cold_diags:?}"));
        assert!(warm.refinement_dependencies.contains_key("b_unrelated"));
        // A later edit must rediscover the alias attachment even when the
        // loader is not in that edit's dependency closure.
        warm.update_file(
            "callers.R".into(),
            parse_file("callers.R", &callers.replace("1L", "2L")).into(),
        );
        assert_matches_cold(&mut warm);
        warm.update_file("loader.R".into(), parse_file("loader.R", before).into());
        assert_matches_cold(&mut warm);
        assert!(!warm.refinement_discovered_attachments);
    }

    #[test]
    fn empty_project_has_no_diagnostics() {
        let mut project = Project::new();
        let diags = project.check();
        assert!(diags.is_empty(), "empty project should have no diags");
    }

    #[test]
    fn single_file_via_project_matches_checker() {
        // A single-file Project behaves like a single-file Checker.
        let src = "f <- function() { \"hello\" }\ny <- f() + 1L\n";
        let file = parse_file("a.R", src);

        let mut project = Project::new();
        project.add_file("a.R".to_string(), file);
        let diags = project.check();
        let all: Vec<_> = diags.into_iter().flat_map(|(_, d)| d).collect();
        assert!(
            all.iter().any(|d| d.code == "RY040"),
            "expected RY040 from char fn + int, got {:?}",
            all
        );
    }

    #[test]
    fn shiny_fragment_paths_bind_server_ambient_names() {
        let mut project = Project::new();
        project.add_file(
            "inst/shiny/src/server/fragment.R".to_string(),
            parse_file(
                "inst/shiny/src/server/fragment.R",
                "output$value <- input$value\nsession$sendCustomMessage('x', list())\n",
            ),
        );
        let diagnostics: Vec<_> = project
            .check()
            .into_iter()
            .flat_map(|(_, diagnostics)| diagnostics)
            .collect();
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "RY010"),
            "Shiny fragments must receive input/output/session: {diagnostics:?}"
        );
    }

    #[test]
    fn loaded_package_eval_metadata_applies_to_project_functions() {
        let mut project = Project::new();
        project.add_file(
            "function.R".to_string(),
            parse_file("function.R", "list.map <- function(.data, expr) expr\n"),
        );
        project.add_file(
            "call.R".to_string(),
            parse_file("call.R", "r <- list.map(some_list(), . + score)\n"),
        );
        project.set_loaded(std::collections::HashSet::from(["rlist".to_string()]));
        let diagnostics: Vec<_> = project
            .check()
            .into_iter()
            .flat_map(|(_, diagnostics)| diagnostics)
            .collect();
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "RY010"),
            "project calls should honor loaded stub eval metadata: {diagnostics:?}"
        );
    }

    #[test]
    fn scope_capture_records_top_and_function_scopes_once() {
        use crate::ScopeRecordKind;

        // A nested closure: the inner body references `base`, which only
        // the outer scope binds, so the inner snapshot must still contain
        // it (R's lexical capture) while the outer records it as a local.
        // The trailing `outer()` omits both formals, which is the call
        // evidence that lets ry commit to `x`'s default type. Without a
        // call site a defaulted formal stays opaque by design.
        let src = "base <- 2L\nouter <- function(x = 1L, y) {\n  local <- x + base\n  inner <- function(z) z + base\n  inner(y)\n}\nouter()\n";
        let mut project = Project::new();
        project.add_file("a.R".to_string(), parse_file("a.R", src));
        project.enable_scope_capture();
        project.check();
        let records = project.take_scope_records();
        assert_eq!(records.len(), 1, "one file: {records:?}");
        let (path, records) = &records[0];
        assert_eq!(path, "a.R");

        let mut sorted = records.clone();
        sorted.sort_by_key(|record| record.span.start);
        // top, outer, inner -- exactly one record each (the fixpoint and
        // signature walks must not double-capture).
        assert_eq!(sorted.len(), 3, "{sorted:?}");
        assert_eq!(sorted[0].kind, ScopeRecordKind::Top);
        assert!(sorted[0].name.is_none());
        assert_eq!(sorted[1].kind, ScopeRecordKind::Function);
        assert_eq!(sorted[1].name.as_deref(), Some("outer"));
        assert_eq!(
            sorted[1]
                .params
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            vec!["x", "y"]
        );
        assert_eq!(sorted[2].name.as_deref(), Some("inner"));

        let outer = &sorted[1];
        // The default-valued parameter carries its literal type. The
        // default-less one stays opaque.
        assert_eq!(
            outer.scope.get("x").map(|t| t.to_string()),
            Some("integer<len=1>".to_string())
        );
        assert!(outer.scope.parameter_bindings.contains("x"));
        // Captured from the outer scope's cloned table.
        assert!(sorted[2].scope.get("base").is_some());
        // Local assignment present in the final snapshot.
        assert!(outer.scope.get("local").is_some());
        // No capture without opting in.
        let mut plain = Project::new();
        plain.add_file("a.R".to_string(), parse_file("a.R", src));
        plain.check();
        assert!(plain.take_scope_records().is_empty());
    }

    /// A package whose `R/` files carry `source_package` facts, plus one
    /// tinytest file outside `R/`.
    fn package_with_tinytest(def: &str, test: &str) -> Project {
        let mut project = Project::new();
        project.set_package_facts(
            ["R/def.R", "R/use.R"]
                .map(|path| {
                    let facts = ry_workspace::PackageFacts {
                        source_package: Some("mypkg".into()),
                        ..Default::default()
                    };
                    (path.to_string(), facts)
                })
                .into(),
        );
        project.add_file("R/def.R".into(), parse_file("R/def.R", def));
        project.add_file(
            "R/use.R".into(),
            parse_file("R/use.R", "f <- function() c(in_ns, leaked)\n"),
        );
        project.add_file(
            "inst/tinytest/test_a.R".into(),
            parse_file("inst/tinytest/test_a.R", test),
        );
        project
    }

    fn unbound_names(diagnostics: &[(String, Vec<Diagnostic>)], path: &str) -> Vec<String> {
        diagnostics
            .iter()
            .filter(|(found, _)| found == path)
            .flat_map(|(_, found)| found)
            .filter(|d| d.code == "RY010")
            .map(|d| d.message.clone())
            .collect()
    }

    #[test]
    fn package_r_code_does_not_see_bindings_from_files_outside_r() {
        // R builds the namespace from `R/` only; a tinytest file runs in
        // its own environment, so its top-level `leaked` never binds in
        // `f`. The test file still sees the namespace's `in_ns`.
        // A `<<-` there writes outside the namespace too.
        for test in [
            "leaked <- 1L\nin_ns + leaked\n",
            "h <- function() leaked <<- 1L\nin_ns\n",
        ] {
            let mut project = package_with_tinytest("in_ns <- 1L\n", test);
            let diagnostics = project.check();
            let use_r = unbound_names(&diagnostics, "R/use.R");
            assert_eq!(use_r.len(), 1, "{test:?}: {use_r:?}");
            assert!(use_r[0].contains("`leaked`"), "{test:?}: {use_r:?}");
            assert!(unbound_names(&diagnostics, "inst/tinytest/test_a.R").is_empty());
        }
        // Control: a `<<-` in another `R/` file still binds for package code.
        let mut project =
            package_with_tinytest("in_ns <- 1L\ng <- function() leaked <<- 1L\n", "in_ns\n");
        assert!(unbound_names(&project.check(), "R/use.R").is_empty());
    }

    #[test]
    fn removing_a_namespace_binding_re_emits_r_files_when_a_test_still_binds_it() {
        // Dropping `in_ns` from `R/def.R` leaves the project-wide pool
        // unchanged (the test file binds it too), but `R/use.R` must still
        // re-emit against the smaller namespace.
        let mut project = package_with_tinytest("in_ns <- 1L\nleaked <- 1L\n", "in_ns <- 1L\n");
        assert!(unbound_names(&project.check(), "R/use.R").is_empty());
        project.update_file(
            "R/def.R".into(),
            parse_file("R/def.R", "leaked <- 1L\n").into(),
        );
        let use_r = unbound_names(&project.check_incremental(), "R/use.R");
        assert_eq!(use_r.len(), 1, "{use_r:?}");
        assert!(use_r[0].contains("`in_ns`"), "{use_r:?}");
    }
}
