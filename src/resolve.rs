//! Invocation-local evaluation of named values, resource targets, and scripts.
use crate::{
    script::{Argument, Script},
    store::{Entry, State},
    template::{self, Expression},
};
use anyhow::{Context, Result, bail, ensure};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct Input {
    id: String,
    argument: Argument,
}

fn owner(state: &State, entry: &Entry) -> usize {
    state
        .sources
        .iter()
        .position(|source| {
            source
                .entries
                .iter()
                .any(|candidate| candidate.key == entry.key)
        })
        .unwrap()
}

/// References use absolute source names. Local definitions retain their identity
/// when collision handling renames their exposed command namespace.
fn reference(state: &State, entry: &Entry, path: &[String]) -> Vec<String> {
    let source = &state.sources[owner(state, entry)];
    let mapped = crate::import::map_key(path, &source.renames, false);
    if source
        .entries
        .iter()
        .any(|candidate| candidate.key.starts_with(&mapped))
    {
        mapped
    } else {
        path.to_vec()
    }
}

fn input_for(entry: &Entry) -> Argument {
    let choices = entry.choices.as_ref().unwrap();
    Argument {
        name: entry.key.join("."),
        hint: entry.hint.clone(),
        choices: Some(choices.iter().map(|choice| choice.value.clone()).collect()),
        choice_hints: choices.iter().map(|choice| choice.hint.clone()).collect(),
        ..Default::default()
    }
}

fn free_id(entry: &Entry, name: &str) -> String {
    format!("{}:{name}", entry.key.join(" "))
}

fn argument_input(state: &State, entry: &Entry, argument: &Argument) -> Result<Input> {
    if let Some(name) = &argument.reference {
        let Expression::Path(path) = template::expression(name)? else {
            bail!("Argument references must name a list or record group: {name}");
        };
        let key = reference(state, entry, &path);
        let mut input = if let Ok(target) = state.lookup(&key) {
            ensure!(
                target.choices.is_some(),
                "Argument reference {name} must name a list or record group"
            );
            input_for(target)
        } else {
            ensure!(
                !argument.multiple,
                "Multiple choice argument {} must reference a list, not a record group",
                argument.name
            );
            // Include empty groups so malformed records cannot silently disappear
            // from the choices inferred from their fields.
            let children = state
                .children(&key)
                .into_iter()
                .chain(
                    state
                        .sources
                        .iter()
                        .flat_map(|source| &source.groups)
                        .filter(|group| {
                            group.key.starts_with(&key) && group.key.len() == key.len() + 1
                        })
                        .map(|group| group.key[key.len()].clone()),
                )
                .collect::<BTreeSet<_>>();
            ensure!(
                !children.is_empty(),
                "Argument reference {name} must name a nonempty list or record group"
            );
            let mut choices = vec![];
            let mut choice_hints = vec![];
            for child in children {
                let mut record = key.clone();
                record.push(child.clone());
                ensure!(
                    state.lookup(&record).is_err() && state.contains(&record),
                    "Argument reference {name}: {} must be a nonempty record",
                    record.join(".")
                );
                choices.push(child);
                choice_hints.push(state.hint(&record).map(str::to_owned));
            }
            Argument {
                hint: state.hint(&key).map(str::to_owned),
                choices: Some(choices),
                choice_hints,
                ..Default::default()
            }
        };
        input.name = argument.name.clone();
        input.multiple = argument.multiple;
        input.join = argument.join.clone();
        crate::script::validate_arguments(Some(std::slice::from_ref(&input)))?;
        Ok(Input {
            id: if argument.multiple {
                // A control separator cannot collide with a resource key or
                // a scalar input. Different joins keep distinct selections.
                format!("{}\0{}", key.join(" "), argument.join.as_deref().unwrap())
            } else {
                key.join(" ")
            },
            argument: input,
        })
    } else {
        Ok(Input {
            id: free_id(entry, &argument.name),
            argument: argument.clone(),
        })
    }
}

fn arguments(entry: &Entry) -> Option<&[Argument]> {
    entry
        .script
        .as_ref()
        .map_or(entry.args.as_deref(), |script| script.args.as_deref())
}

/// Bind argument field access to the existing lookup evaluator. For example,
/// project.org selects projects[project, "org"], with a fixed final key.
fn expression(state: &State, entry: &Entry, text: &str) -> Result<Expression> {
    fn bind(state: &State, entry: &Entry, expr: Expression) -> Result<Expression> {
        match expr {
            Expression::Path(path) => {
                let Some(argument) = arguments(entry)
                    .and_then(|args| args.iter().find(|arg| arg.name == path[0]))
                    .filter(|_| path.len() > 1)
                else {
                    return Ok(Expression::Path(path));
                };
                let name = argument.reference.as_deref().with_context(|| {
                    format!(
                        "Argument {} needs a record reference to access fields",
                        argument.name
                    )
                })?;
                let Expression::Path(root) = template::expression(name)? else {
                    bail!("Argument references must name a list or record group: {name}");
                };
                let key = reference(state, entry, &root);
                ensure!(
                    state.lookup(&key).is_err(),
                    "Argument {} needs a record group to access fields",
                    argument.name
                );
                let input = argument_input(state, entry, argument)?;
                for choice in input.argument.choices.as_ref().unwrap() {
                    let target = key
                        .iter()
                        .cloned()
                        .chain(std::iter::once(choice.clone()))
                        .chain(path[1..].iter().cloned())
                        .collect::<Vec<_>>();
                    ensure!(
                        state.contains(&target),
                        "Missing record field: {} (argument {})",
                        target.join("."),
                        argument.name
                    );
                }
                let selectors = std::iter::once(Expression::Path(vec![argument.name.clone()]))
                    .chain(path[1..].iter().cloned().map(Expression::Key))
                    .collect();
                Ok(Expression::Lookup(
                    Box::new(Expression::Path(root)),
                    selectors,
                ))
            }
            Expression::Lookup(base, selectors) => {
                let base = bind(state, entry, *base)?;
                let selectors = selectors
                    .into_iter()
                    .map(|selector| bind(state, entry, selector))
                    .collect::<Result<Vec<_>>>()?;
                // A record field can itself be a mapping, e.g. service.hosts[env].
                if let Expression::Lookup(root, mut fields) = base {
                    fields.extend(selectors);
                    Ok(Expression::Lookup(root, fields))
                } else {
                    Ok(Expression::Lookup(Box::new(base), selectors))
                }
            }
            Expression::Key(_) => Ok(expr),
        }
    }
    bind(state, entry, template::expression(text)?)
}

struct Plan<'a> {
    state: &'a State,
    supplied: &'a [String],
    inputs: Vec<Input>,
    active: Vec<Vec<String>>,
    complete: BTreeSet<Vec<String>>,
    uncertain_from: Option<usize>,
}

impl<'a> Plan<'a> {
    fn new(state: &'a State, supplied: &'a [String]) -> Self {
        Self {
            state,
            supplied,
            inputs: vec![],
            active: vec![],
            complete: BTreeSet::new(),
            uncertain_from: None,
        }
    }

    fn add(&mut self, input: Input) -> Result<()> {
        if self.inputs.iter().any(|candidate| candidate.id == input.id) {
            return Ok(());
        }
        if self.uncertain_from.is_none()
            && let Some(value) = self.supplied.get(self.inputs.len())
        {
            input.argument.validate_value(value)?;
        }
        self.inputs.push(input);
        Ok(())
    }

    fn node(&mut self, entry: &Entry) -> Result<()> {
        if self.complete.contains(&entry.key) {
            return Ok(());
        }
        ensure!(
            !self.active.contains(&entry.key),
            "Circular reference: {} -> {}",
            self.active
                .iter()
                .map(|key| key.join("."))
                .collect::<Vec<_>>()
                .join(" -> "),
            entry.key.join(".")
        );
        ensure!(self.active.len() < 128, "Reference chain exceeds 128 nodes");
        self.active.push(entry.key.clone());
        if entry.choices.is_some() {
            self.add(Input {
                id: entry.key.join(" "),
                argument: input_for(entry),
            })?;
        } else {
            if let Some(arguments) = arguments(entry) {
                for argument in arguments {
                    self.add(argument_input(self.state, entry, argument)?)?;
                }
            }
            if let Some(script) = &entry.script {
                for value in script.env.values() {
                    self.text(entry, value)?;
                }
            } else {
                self.text(entry, entry.value.as_ref().unwrap_or(&entry.url))?;
            }
        }
        self.active.pop();
        self.complete.insert(entry.key.clone());
        Ok(())
    }

    fn text(&mut self, entry: &Entry, text: &str) -> Result<()> {
        for name in template::names(text)? {
            self.expr(entry, &expression(self.state, entry, &name)?)?;
        }
        Ok(())
    }

    fn expr(&mut self, entry: &Entry, expr: &Expression) -> Result<()> {
        match expr {
            Expression::Key(_) => Ok(()),
            Expression::Path(path) => {
                if path.len() == 1
                    && let Some(argument) = arguments(entry)
                        .and_then(|args| args.iter().find(|arg| arg.name == path[0]))
                {
                    return self.add(argument_input(self.state, entry, argument)?);
                }
                let key = reference(self.state, entry, path);
                if let Ok(target) = self.state.lookup(&key) {
                    return self.node(target);
                }
                ensure!(
                    !self.state.contains(&key),
                    "Group {} requires a child or lookup",
                    path.join(".")
                );
                ensure!(
                    path.len() == 1 && template::valid_name(&path[0]),
                    "Unknown reference: {}",
                    path.join(".")
                );
                self.add(Input {
                    id: free_id(entry, &path[0]),
                    argument: Argument {
                        name: path[0].clone(),
                        ..Default::default()
                    },
                })
            }
            Expression::Lookup(base, selectors) => {
                let Expression::Path(path) = base.as_ref() else {
                    bail!("Lookup requires a named group");
                };
                let root = reference(self.state, entry, path);
                ensure!(
                    self.state.contains(&root),
                    "Unknown lookup group: {}",
                    path.join(".")
                );
                for selector in selectors {
                    self.expr(entry, selector)?;
                }
                let selected = selectors
                    .iter()
                    .map(|selector| self.known(entry, selector, &mut BTreeSet::new()))
                    .collect::<Result<Vec<_>>>()?;
                if selected.iter().any(Option::is_none) && self.supplied.len() > self.inputs.len() {
                    self.uncertain_from.get_or_insert(self.inputs.len());
                }
                // Unknown choices are explored for validation only; execution will
                // evaluate just the selected branch, without prompting unused inputs.
                let candidates = self
                    .state
                    .sources
                    .iter()
                    .flat_map(|source| &source.entries)
                    .filter(|candidate| {
                        candidate.key.starts_with(&root)
                            && candidate.key.len() == root.len() + selectors.len()
                    })
                    .filter(|candidate| {
                        selected.iter().enumerate().all(|(index, value)| {
                            value
                                .as_ref()
                                .is_none_or(|value| candidate.key[root.len() + index] == *value)
                        })
                    })
                    .collect::<Vec<_>>();
                ensure!(
                    !candidates.is_empty(),
                    "Missing lookup combination for {}",
                    path.join(".")
                );
                for candidate in candidates {
                    self.node(candidate)?;
                }
                Ok(())
            }
        }
    }

    /// Inspect supplied/fixed values without running scripts or opening prompts.
    fn known(
        &self,
        entry: &Entry,
        expr: &Expression,
        active: &mut BTreeSet<Vec<String>>,
    ) -> Result<Option<String>> {
        match expr {
            Expression::Key(key) => Ok(Some(key.clone())),
            Expression::Path(path) => {
                if path.len() == 1
                    && let Some(argument) = arguments(entry)
                        .and_then(|args| args.iter().find(|arg| arg.name == path[0]))
                {
                    let input = argument_input(self.state, entry, argument)?;
                    return Ok(self
                        .inputs
                        .iter()
                        .position(|candidate| candidate.id == input.id)
                        .filter(|index| self.uncertain_from.is_none_or(|start| *index < start))
                        .and_then(|index| self.supplied.get(index))
                        .cloned());
                }
                let key = reference(self.state, entry, path);
                if let Ok(target) = self.state.lookup(&key) {
                    if target.choices.is_some() {
                        return Ok(self
                            .inputs
                            .iter()
                            .position(|input| input.id == key.join(" "))
                            .filter(|index| self.uncertain_from.is_none_or(|start| *index < start))
                            .and_then(|index| self.supplied.get(index))
                            .cloned());
                    }
                    if target.script.is_some() {
                        return Ok(None);
                    }
                    ensure!(
                        active.insert(key.clone()),
                        "Circular reference: {}",
                        key.join(".")
                    );
                    let text = target.value.as_ref().unwrap_or(&target.url);
                    // URL references may encode their choices. Leave their exact
                    // lookup key to the evaluator rather than guessing raw text.
                    if crate::resource::is_url(text) && text.contains('{') {
                        active.remove(&key);
                        return Ok(None);
                    }
                    let mut result = String::new();
                    let mut rest = text.as_str();
                    while let Some(start) = rest.find('{') {
                        result.push_str(&rest[..start]);
                        let end = rest[start..].find('}').unwrap() + start;
                        let Some(value) = self.known(
                            target,
                            &expression(self.state, target, &rest[start + 1..end])?,
                            active,
                        )?
                        else {
                            active.remove(&key);
                            return Ok(None);
                        };
                        result.push_str(&value);
                        rest = &rest[end + 1..];
                    }
                    result.push_str(rest);
                    active.remove(&key);
                    return Ok(Some(result));
                }
                Ok(self
                    .inputs
                    .iter()
                    .position(|input| input.id == free_id(entry, &path[0]))
                    .filter(|index| self.uncertain_from.is_none_or(|start| *index < start))
                    .and_then(|index| self.supplied.get(index))
                    .cloned())
            }
            Expression::Lookup(base, selectors) => {
                let Expression::Path(path) = base.as_ref() else {
                    unreachable!()
                };
                let mut key = reference(self.state, entry, path);
                for selector in selectors {
                    let Some(value) = self.known(entry, selector, active)? else {
                        return Ok(None);
                    };
                    key.push(value);
                }
                if let Ok(target) = self.state.lookup(&key) {
                    // Resolve in the target's source scope without reparsing selected keys.
                    if target.script.is_some() {
                        return Ok(None);
                    }
                    if let Some(value) = &target.value
                        && !value.contains('{')
                    {
                        return Ok(Some(value.clone()));
                    }
                    if !target.url.contains('{') && !target.url.is_empty() {
                        return Ok(Some(target.url.clone()));
                    }
                }
                Ok(None)
            }
        }
    }
}

/// Validate dependency graphs after all imports and collision renames are known.
pub fn validate(state: &State) -> Result<()> {
    let mut plan = Plan::new(state, &[]);
    for entry in state.sources.iter().flat_map(|source| &source.entries) {
        plan.node(entry)
            .with_context(|| format!("At {}", entry.key.join(".")))?;
    }
    Ok(())
}

#[derive(Clone)]
struct Value {
    text: String,
    raw: bool,
    parts: Vec<Value>,
}

pub struct Evaluation {
    pub value: String,
    pub script: Option<Script>,
    pub arguments: Vec<String>,
}

pub fn evaluate(
    state: &State,
    entry: &Entry,
    supplied: &[String],
    prompt: impl FnMut(&Argument) -> Result<String>,
) -> Result<Evaluation> {
    let mut plan = Plan::new(state, supplied);
    plan.node(entry)?;
    let legacy = entry
        .script
        .as_ref()
        .is_some_and(|script| script.args.is_none())
        && plan.inputs.is_empty();
    if !legacy {
        ensure!(
            supplied.len() <= plan.inputs.len(),
            "Expected {} argument(s), got {}",
            plan.inputs.len(),
            supplied.len()
        );
    }
    let mut evaluator = Evaluator {
        state,
        inputs: plan.inputs,
        supplied: if legacy { &[] } else { supplied },
        prompt,
        values: BTreeMap::new(),
        selected: BTreeMap::new(),
        active: BTreeSet::new(),
        used: BTreeSet::new(),
        next: 0,
        previews: BTreeMap::new(),
        deferred: false,
    };
    evaluator.prepare(entry)?;
    evaluator.deferred = false;
    let mut result = Evaluation {
        value: String::new(),
        script: None,
        arguments: vec![],
    };
    if let Some(script) = &entry.script {
        let (script, arguments) = evaluator.script(entry, script)?;
        result.script = Some(script);
        result.arguments = if legacy { supplied.to_vec() } else { arguments };
    } else {
        result.value = evaluator.node(entry)?.text;
    }
    ensure!(
        evaluator
            .supplied
            .iter()
            .enumerate()
            .all(|(index, _)| evaluator.used.contains(&index)),
        "Unexpected arguments for selected lookup branch"
    );
    Ok(result)
}

struct Evaluator<'a, F> {
    state: &'a State,
    inputs: Vec<Input>,
    supplied: &'a [String],
    prompt: F,
    values: BTreeMap<Vec<String>, Value>,
    selected: BTreeMap<String, String>,
    active: BTreeSet<Vec<String>>,
    used: BTreeSet<usize>,
    next: usize,
    previews: BTreeMap<Vec<String>, Option<Value>>,
    deferred: bool,
}

impl<F: FnMut(&Argument) -> Result<String>> Evaluator<'_, F> {
    fn input(&mut self, id: &str) -> Result<String> {
        if let Some(value) = self.selected.get(id) {
            return Ok(value.clone());
        }
        let argument = &self
            .inputs
            .iter()
            .find(|input| input.id == id)
            .context("Unplanned input")?
            .argument;
        let index = self.next;
        self.next += 1;
        let value = match self.supplied.get(index) {
            Some(value) => {
                self.used.insert(index);
                value.clone()
            }
            None => (self.prompt)(argument)?,
        };
        argument.validate_value(&value)?;
        self.selected.insert(id.to_owned(), value.clone());
        Ok(value)
    }

    fn prepare(&mut self, entry: &Entry) -> Result<Option<Value>> {
        if self.deferred {
            return Ok(None);
        }
        if let Some(value) = self.previews.get(&entry.key) {
            return Ok(value.clone());
        }
        let value = if entry.choices.is_some() {
            Some(Value {
                text: self.input(&entry.key.join(" "))?,
                raw: false,
                parts: vec![],
            })
        } else {
            if let Some(arguments) = arguments(entry) {
                for argument in arguments {
                    self.input(&argument_input(self.state, entry, argument)?.id)?;
                }
            }
            if let Some(script) = &entry.script {
                for value in script.env.values() {
                    self.preview_text(entry, value, false)?;
                    if self.deferred {
                        break;
                    }
                }
                None
            } else {
                self.preview_text(entry, entry.value.as_ref().unwrap_or(&entry.url), true)?
            }
        };
        if !self.deferred {
            self.previews.insert(entry.key.clone(), value.clone());
        }
        Ok(value)
    }

    fn preview_expr(&mut self, entry: &Entry, expr: &Expression) -> Result<Option<Value>> {
        match expr {
            Expression::Key(key) => Ok(Some(Value {
                text: key.clone(),
                raw: true,
                parts: vec![],
            })),
            Expression::Path(path) => {
                if path.len() == 1
                    && let Some(argument) = arguments(entry)
                        .and_then(|args| args.iter().find(|arg| arg.name == path[0]))
                {
                    return Ok(Some(Value {
                        text: self.input(&argument_input(self.state, entry, argument)?.id)?,
                        raw: false,
                        parts: vec![],
                    }));
                }
                let key = reference(self.state, entry, path);
                if let Ok(target) = self.state.lookup(&key) {
                    self.prepare(target)
                } else {
                    Ok(Some(Value {
                        text: self.input(&free_id(entry, &path[0]))?,
                        raw: false,
                        parts: vec![],
                    }))
                }
            }
            Expression::Lookup(base, selectors) => {
                let Expression::Path(path) = base.as_ref() else {
                    unreachable!()
                };
                let mut key = reference(self.state, entry, path);
                let mut unknown = false;
                for selector in selectors {
                    if let Some(value) = self.preview_expr(entry, selector)? {
                        key.push(value.text);
                    } else {
                        unknown = true;
                    }
                    if self.deferred {
                        return Ok(None);
                    }
                }
                if unknown {
                    self.deferred = true;
                    return Ok(None);
                }
                let target = self
                    .state
                    .lookup(&key)
                    .with_context(|| format!("Missing lookup combination: {}", key.join(".")))?;
                self.prepare(target)
            }
        }
    }

    fn preview_text(
        &mut self,
        entry: &Entry,
        text: &str,
        encode_url: bool,
    ) -> Result<Option<Value>> {
        let mut parts = vec![];
        let mut unknown = false;
        let mut rest = text;
        while let Some(start) = rest.find('{') {
            parts.push(Value {
                text: rest[..start].to_owned(),
                raw: true,
                parts: vec![],
            });
            let end = rest[start..].find('}').context("Unclosed reference")? + start;
            if let Some(value) = self.preview_expr(
                entry,
                &expression(self.state, entry, &rest[start + 1..end])?,
            )? {
                parts.push(value);
            } else {
                unknown = true;
            }
            if self.deferred {
                return Ok(None);
            }
            rest = &rest[end + 1..];
        }
        parts.push(Value {
            text: rest.to_owned(),
            raw: true,
            parts: vec![],
        });
        if unknown {
            Ok(None)
        } else {
            Ok(Some(Self::composed(parts, encode_url)?))
        }
    }

    fn node(&mut self, entry: &Entry) -> Result<Value> {
        if let Some(value) = self.values.get(&entry.key) {
            return Ok(value.clone());
        }
        ensure!(
            self.active.insert(entry.key.clone()),
            "Circular reference: {}",
            entry.key.join(".")
        );
        let value = if entry.choices.is_some() {
            Value {
                text: self.input(&entry.key.join(" "))?,
                raw: false,
                parts: vec![],
            }
        } else if let Some(script) = &entry.script {
            let (script, arguments) = self.script(entry, script)?;
            Value {
                text: script
                    .capture(&arguments)
                    .with_context(|| format!("At {}", entry.key.join(".")))?,
                raw: false,
                parts: vec![],
            }
        } else {
            let parts = self.parts(entry, entry.value.as_ref().unwrap_or(&entry.url))?;
            Self::composed(parts, true)?
        };
        self.active.remove(&entry.key);
        self.values.insert(entry.key.clone(), value.clone());
        Ok(value)
    }

    fn expr(&mut self, entry: &Entry, expr: &Expression) -> Result<Value> {
        match expr {
            Expression::Key(key) => Ok(Value {
                text: key.clone(),
                raw: true,
                parts: vec![],
            }),
            Expression::Path(path) => {
                if path.len() == 1
                    && let Some(argument) = arguments(entry)
                        .and_then(|args| args.iter().find(|arg| arg.name == path[0]))
                {
                    return Ok(Value {
                        text: self.input(&argument_input(self.state, entry, argument)?.id)?,
                        raw: false,
                        parts: vec![],
                    });
                }
                let key = reference(self.state, entry, path);
                if let Ok(target) = self.state.lookup(&key) {
                    self.node(target)
                } else {
                    Ok(Value {
                        text: self.input(&free_id(entry, &path[0]))?,
                        raw: false,
                        parts: vec![],
                    })
                }
            }
            Expression::Lookup(base, selectors) => {
                let Expression::Path(path) = base.as_ref() else {
                    unreachable!()
                };
                let mut key = reference(self.state, entry, path);
                for selector in selectors {
                    key.push(self.expr(entry, selector)?.text);
                }
                let target = self
                    .state
                    .lookup(&key)
                    .with_context(|| format!("Missing lookup combination: {}", key.join(".")))?;
                self.prepare(target)?;
                self.deferred = false;
                self.node(target)
            }
        }
    }

    fn text(&mut self, entry: &Entry, text: &str, encode_url: bool) -> Result<String> {
        let parts = self.parts(entry, text)?;
        Self::join(parts, encode_url)
    }

    fn parts(&mut self, entry: &Entry, text: &str) -> Result<Vec<Value>> {
        let mut parts = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find('{') {
            parts.push(Value {
                text: rest[..start].to_owned(),
                raw: true,
                parts: vec![],
            });
            let end = rest[start..].find('}').context("Unclosed reference")? + start;
            parts.push(self.expr(
                entry,
                &expression(self.state, entry, &rest[start + 1..end])?,
            )?);
            rest = &rest[end + 1..];
        }
        parts.push(Value {
            text: rest.to_owned(),
            raw: true,
            parts: vec![],
        });
        Ok(parts)
    }

    fn composed(parts: Vec<Value>, encode_url: bool) -> Result<Value> {
        let text = Self::join(parts.clone(), encode_url)?;
        let finalized = encode_url && crate::resource::is_url(&text);
        Ok(Value {
            text,
            raw: true,
            parts: if finalized { vec![] } else { parts },
        })
    }

    fn join(parts: Vec<Value>, encode_url: bool) -> Result<String> {
        fn flatten(parts: Vec<Value>, leaves: &mut Vec<Value>) {
            for part in parts {
                if part.parts.is_empty() {
                    leaves.push(part);
                } else {
                    flatten(part.parts, leaves);
                }
            }
        }
        let mut leaves = Vec::new();
        flatten(parts, &mut leaves);
        let parts = leaves;
        let url = encode_url
            && crate::resource::is_url(
                &parts
                    .iter()
                    .map(|part| part.text.as_str())
                    .collect::<String>(),
            );
        let mut result = String::new();
        let mut at_start = true;
        for part in parts {
            let target_prefix = at_start && crate::resource::is_url(&part.text);
            if !part.text.is_empty() {
                at_start = false;
            }
            if url && !part.raw && !target_prefix {
                use std::fmt::Write;
                for byte in part.text.bytes() {
                    if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                        result.push(char::from(byte));
                    } else {
                        write!(result, "%{byte:02X}")?;
                    }
                }
            } else {
                result.push_str(&part.text);
            }
        }
        Ok(result)
    }

    fn script(&mut self, entry: &Entry, script: &Script) -> Result<(Script, Vec<String>)> {
        let mut resolved = script.clone();
        let mut values = Vec::new();
        if let Some(arguments) = &script.args {
            for argument in arguments {
                values.push(self.input(&argument_input(self.state, entry, argument)?.id)?);
            }
            resolved.args = Some(
                arguments
                    .iter()
                    .map(|argument| Argument {
                        name: argument.name.clone(),
                        ..Default::default()
                    })
                    .collect(),
            );
        }
        for value in resolved.env.values_mut() {
            *value = self.text(entry, value, false)?;
            if value == "~" || value.starts_with("~/") {
                *value = crate::resource::normalize(value)?;
            }
        }
        Ok((resolved, values))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, fs};

    fn fixture(text: &str) -> (tempfile::TempDir, State) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tools.toml");
        fs::write(&path, text).unwrap();
        let state = State {
            sources: vec![crate::import::read(&path, BTreeMap::new()).unwrap()],
            ..Default::default()
        };
        validate(&state).unwrap();
        (dir, state)
    }

    fn entry<'a>(state: &'a State, key: &[&str]) -> &'a Entry {
        state
            .lookup(&key.iter().map(|key| key.to_string()).collect::<Vec<_>>())
            .unwrap()
    }

    #[test]
    fn prompts_carry_list_and_choice_hints_and_repeated_references_share_selection() {
        let (_dir, state) = fixture(
            r#"
[projects]
hidden=true
org='qrlkit'
[projects.repos]
hint='Choose a project'
values=[{value='rs',hint='Rust CLI'},{value='web',hint='Website'}]
[repos.open]
url='https://github.com/{projects.org}/{projects.repos}'
[repos.prs]
hint='Pull requests'
url='{repos.open}/pulls?repo={projects.repos}'
"#,
        );
        let mut prompts = 0;
        let result = evaluate(&state, entry(&state, &["repos", "prs"]), &[], |argument| {
            prompts += 1;
            assert_eq!(argument.hint.as_deref(), Some("Choose a project"));
            assert_eq!(argument.choices.as_ref().unwrap(), &["rs", "web"]);
            assert_eq!(
                argument.choice_hints,
                [Some("Rust CLI".into()), Some("Website".into())]
            );
            Ok("web".into())
        })
        .unwrap();
        assert_eq!(prompts, 1);
        assert_eq!(result.value, "https://github.com/qrlkit/web/pulls?repo=web");
        assert_eq!(
            entry(&state, &["repos", "prs"]).hint.as_deref(),
            Some("Pull requests")
        );
    }

    #[test]
    fn multiple_choice_references_share_only_the_same_selection_mode_and_separator() {
        let (_dir, state) = fixture(
            r#"
[shared.regions]
hint='Choose regions'
values=[{value='eu',hint='Europe'},{value='us',hint='United States'},{value='asia',hint='Asia'}]
[view]
args=[
  {name='regions',enum={ref='shared.regions'},multiple=true,join=','},
  {name='mirror',enum={ref='shared.regions'},multiple=true,join=','},
  {name='single',enum={ref='shared.regions'}},
  {name='other',enum={ref='shared.regions'},multiple=true,join='|'},
]
url='https://example.com/{regions}/{mirror}/{single}/{other}/{shared.regions}'
"#,
        );
        let mut prompts = vec![];
        let result = evaluate(&state, entry(&state, &["view"]), &[], |argument| {
            prompts.push(argument.name.clone());
            assert_eq!(argument.hint.as_deref(), Some("Choose regions"));
            assert_eq!(argument.choices.as_ref().unwrap(), &["eu", "us", "asia"]);
            assert_eq!(argument.choice_hints[0].as_deref(), Some("Europe"));
            Ok(match argument.join.as_deref() {
                Some(",") => "eu,asia",
                Some("|") => "eu|asia",
                None => "us",
                _ => unreachable!(),
            }
            .into())
        })
        .unwrap();
        assert_eq!(prompts, ["regions", "single", "other"]);
        assert_eq!(
            result.value,
            "https://example.com/eu%2Casia/eu%2Casia/us/eu%7Casia/us"
        );
        let supplied = ["eu,asia", "us", "eu|asia"].map(str::to_owned);
        let direct = evaluate(&state, entry(&state, &["view"]), &supplied, |_| {
            panic!("all selections supplied")
        })
        .unwrap();
        assert_eq!(direct.value, result.value);
    }

    #[test]
    fn hidden_subtrees_disappear_from_navigation_but_explicit_navigation_works() {
        let (_dir, state) = fixture(
            "[shared]\nhidden=true\nnames=['rs']\n[repos]\nhint='GitHub'\n[repos.open]\nurl='https://example.com/{shared.names}'\n[repos.internal]\nhidden=true\nvalue='secret'\n",
        );
        assert_eq!(state.visible_children(&[]), ["repos"]);
        assert_eq!(state.visible_children(&["repos".into()]), ["open"]);
        assert_eq!(state.visible_children(&["shared".into()]), ["names"]);
        assert_eq!(state.hint(&["repos".into()]), Some("GitHub"));
    }

    #[test]
    fn cancellation_and_missing_inputs_happen_before_script_dependencies_execute() {
        let (dir, state) = fixture(
            "[dynamic]\nrun='touch captured; printf host'\n[data]\nvalues=['rs','web']\n[logs]\nurl='https://{dynamic}.example.com/{data}'\n",
        );
        let result = evaluate(&state, entry(&state, &["logs"]), &[], |_| {
            bail!("cancelled")
        });
        assert!(result.is_err());
        assert!(!dir.path().join("captured").exists());
    }

    #[test]
    fn branch_choices_follow_actual_selection_and_unused_choices_never_prompt() {
        let (_dir, state) = fixture(
            r#"
[data]
repos=['rs','web']
rust=['cargo','clippy']
web=['npm','bun']
regions=['eu','us']
[logs]
url='https://example.com/{logs.paths[data.repos]}/{data.regions}'
[logs.paths]
rs='{data.rust}'
web='{data.web}'
"#,
        );
        let asked = RefCell::new(Vec::new());
        let answers = RefCell::new(vec!["web", "bun", "eu"].into_iter());
        let result = evaluate(&state, entry(&state, &["logs"]), &[], |argument| {
            asked.borrow_mut().push(argument.name.clone());
            Ok(answers.borrow_mut().next().unwrap().into())
        })
        .unwrap();
        assert_eq!(*asked.borrow(), ["data.repos", "data.web", "data.regions"]);
        assert_eq!(result.value, "https://example.com/bun/eu");
        let result = evaluate(
            &state,
            entry(&state, &["logs"]),
            &["web".into(), "bun".into(), "eu".into()],
            |_| panic!("unexpected prompt"),
        )
        .unwrap();
        assert_eq!(result.value, "https://example.com/bun/eu");
    }

    #[test]
    fn computed_lookup_keys_preserve_branch_specific_argument_order() {
        let (dir, state) = fixture(
            r#"
[selector]
run='printf web; printf x >> calls'
[data]
rust=['cargo','clippy']
web=['npm','bun']
regions=['eu','us']
[logs]
url='https://example.com/{logs.paths[selector]}/{data.regions}?again={selector}'
[logs.paths]
rs='{data.rust}'
web='{data.web}'
"#,
        );
        let result = evaluate(
            &state,
            entry(&state, &["logs"]),
            &["bun".into(), "us".into()],
            |_| panic!("unexpected prompt"),
        )
        .unwrap();
        assert_eq!(result.value, "https://example.com/bun/us?again=web");
        assert_eq!(fs::read_to_string(dir.path().join("calls")).unwrap(), "x");
    }

    #[test]
    fn script_environment_dependencies_share_the_callers_choices_and_cache() {
        let (dir, state) = fixture(
            r#"
[data]
values=['rs','web']
[dynamic]
args=[{name='repo',enum={ref='data'}}]
run='printf "%s" "$1"; printf x >> calls'
[dynamic.env]
REPO='{data}'
[logs]
url='https://example.com/{dynamic}?again={data}&third={dynamic}'
"#,
        );
        let result = evaluate(&state, entry(&state, &["logs"]), &["web".into()], |_| {
            panic!("unexpected prompt")
        })
        .unwrap();
        assert_eq!(result.value, "https://example.com/web?again=web&third=web");
        assert_eq!(fs::read_to_string(dir.path().join("calls")).unwrap(), "x");
    }
    #[test]
    fn freeform_arguments_encode_browser_pages_and_keep_declaration_order() {
        let (_dir, state) = fixture(
            r#"
[page]
url='chrome://extensions/?id={id}&query={query}&again={id}'
args=[{name='query'},{name='id',enum=['a &b']}]
"#,
        );
        let result = evaluate(
            &state,
            entry(&state, &["page"]),
            &["hello world".into(), "a &b".into()],
            |_| panic!("unexpected prompt"),
        )
        .unwrap();
        assert_eq!(
            result.value,
            "chrome://extensions/?id=a%20%26b&query=hello%20world&again=a%20%26b"
        );
    }

    #[test]
    fn record_prompts_reuse_selection_and_only_capture_the_selected_fields() {
        let (dir, state) = fixture(
            r#"
[projects]
hidden=true
hint='Choose a project'
[projects.alpha]
hint='Alpha project'
[projects.alpha.org]
run='printf projects; printf alpha >> captured'
[projects.beta]
hint='Beta project'
[projects.beta.org]
run='printf example; printf beta >> captured'
[browse]
args=[{name='project',enum={ref='projects'}},{name='section',enum=['overview','files','issues','pulls']}]
url='https://example.com/{section}/{project.org}?project={project}&again={project.org}'
"#,
        );
        let browse = entry(&state, &["browse"]);
        let mut asked = vec![];
        let result = evaluate(&state, browse, &[], |argument| {
            asked.push(argument.name.clone());
            if argument.name == "project" {
                assert_eq!(argument.hint.as_deref(), Some("Choose a project"));
                assert_eq!(argument.choices.as_ref().unwrap(), &["alpha", "beta"]);
                assert_eq!(
                    argument.choice_hints,
                    [Some("Alpha project".into()), Some("Beta project".into())]
                );
                Ok("beta".into())
            } else {
                assert!(!dir.path().join("captured").exists());
                Ok("pulls".into())
            }
        })
        .unwrap();
        assert_eq!(asked, ["project", "section"]);
        assert_eq!(
            result.value,
            "https://example.com/pulls/example?project=beta&again=example"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("captured")).unwrap(),
            "beta"
        );
        fs::remove_file(dir.path().join("captured")).unwrap();

        let result = evaluate(&state, browse, &["alpha".into()], |argument| {
            assert_eq!(argument.name, "section");
            bail!("cancelled")
        });
        assert!(result.is_err());
        assert!(!dir.path().join("captured").exists());
        assert!(
            evaluate(&state, browse, &["alpha".into(), "invalid".into()], |_| {
                panic!("invalid input must be rejected before prompts")
            })
            .is_err()
        );
        assert!(!dir.path().join("captured").exists());
    }

    #[test]
    fn record_fields_support_nested_paths_mappings_and_lookup_selectors() {
        let (_dir, state) = fixture(
            r#"
[projects.alpha.routing]
cluster='east'
[projects.alpha.routing.paths]
issues='sandbox'
pulls='production'
[hosts]
east='east.example.com'
[browse]
args=[{name='project',enum={ref='projects'}},{name='section',enum=['issues','pulls']}]
url='https://{hosts[project.routing.cluster]}/{project.routing.paths[section]}'
"#,
        );
        let result = evaluate(
            &state,
            entry(&state, &["browse"]),
            &["alpha".into(), "issues".into()],
            |_| panic!("unexpected prompt"),
        )
        .unwrap();
        assert_eq!(result.value, "https://east.example.com/sandbox");
    }

    #[test]
    fn script_arguments_can_use_record_fields_in_their_environment() {
        let (_dir, state) = fixture(
            r#"
[projects.alpha]
org='projects'
repo='alpha-repo'
[show]
args=[{name='project',enum={ref='projects'}}]
run='printf "%s:%s:%s" "$1" "$ORG" "$REPO"'
[show.env]
ORG='{project.org}'
REPO='{project.repo}'
[open]
url='https://example.com/{show}'
"#,
        );
        let result = evaluate(&state, entry(&state, &["open"]), &["alpha".into()], |_| {
            panic!("unexpected prompt")
        })
        .unwrap();
        assert_eq!(
            result.value,
            "https://example.com/alpha%3Aprojects%3Aalpha-repo"
        );
    }
}
