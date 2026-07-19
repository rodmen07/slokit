# SliPlugin: reusable SLI templates for slokit

- Status: draft, awaiting maintainer review (v0.9.0 PR 1, docs only)
- Date: 2026-07-19 (written against v0.8.0)
- Scope: design for the `SliPlugin` registry and the `sli.plugin` spec key
  promised by the [ROADMAP](../../ROADMAP.md) v0.9.0 milestone
- Decides: definition model, spec surface, API surface, validation and lint
  behavior, and the PR slicing for the implementation (PR 2, blocked on this
  doc merging)

## 1. Summary

Today every SLO spec hand-writes its SLI queries (`events`, `raw`) or uses the
one generated shape (`latency`). Fleets end up copy-pasting the same
availability query with only the `job` selector changed. sloth solved this
with SLI plugins: named, reusable SLI templates referenced from the spec by id
plus options.

This design adds the same capability to slokit:

- A `SliPlugin` trait: a named template that expands typed options into one of
  the existing core `Sli` shapes.
- A `SliPluginRegistry` holding built-in plugins (compiled into the crate) and
  embedder-registered ones.
- A fourth SLI shape in the spec, `sli.plugin: {id, options}`, parse-compatible
  with sloth's key of the same name.
- Validate and lint awareness: unknown ids and broken options are hard errors,
  merely suspicious options are lints, matching the 0.8.0 validation
  philosophy (hard error only when the output would break).

Everything lands behind the existing `spec` feature with zero new
dependencies. The lean core (`--no-default-features`) is untouched.

## 2. Background: how sloth does it

For an honest compatibility story, this is what sloth (the upstream slokit is
spec-compatible with) actually ships:

- A sloth SLI plugin is a **Go source file** interpreted at runtime with the
  Yaegi Go interpreter. The file declares `SLIPluginVersion = "prometheus/v1"`
  and an `SLIPluginID` constant, and exports a
  `SLIPlugin(ctx, meta, labels, options)` function that returns a raw
  error-ratio query string (containing `{{.window}}`).
- Plugins are loaded from directories passed via `--sli-plugins-path`.
- Options arrive as a `map[string]string`; each plugin does its own option
  parsing and erroring.
- Specs reference a plugin as:

  ```yaml
  sli:
    plugin:
      id: "sloth-common/kubernetes/apiserver/availability"
      options:
        filter: cluster="valhalla"
  ```

- A community catalog lives at `github.com/slok/sloth-common-sli-plugins`.

The consequence for slokit: **plugin definition files cannot be compatible.**
Executing sloth's plugins would mean embedding a Go interpreter, which is a
non-starter for a dependency-light Rust crate. What can be compatible is the
**spec surface**: the `sli.plugin` key with `id` and `options` parses
identically, so a sloth spec that uses a plugin is structurally valid slokit
input; whether the referenced id resolves depends on slokit's registry, not
sloth's plugin files.

## 3. Goals and non-goals

### Goals

1. Reusable named SLI templates usable from specs (`sli.plugin`) and from the
   library API.
2. Spec-surface compatibility with sloth: the `sli.plugin: {id, options}` YAML
   shape parses the same way, including `options` as a string map.
3. Typed, declared options with validation that follows the 0.8.0 philosophy:
   hard error when the generated output would be broken or impossible,
   advisory lint when the output loads but is probably not intended.
4. Embedders can register their own plugins without forking slokit, and the
   lean core stays dependency-light and unaffected.
5. Plugin-generated queries flow through the exact same downstream machinery
   (window substitution, validation, generation, promtool checks) as
   hand-written ones. A plugin spec and its hand-written equivalent produce
   byte-identical rules.

### Non-goals

1. **Loading sloth plugin files.** slokit will not execute Go plugin sources,
   with Yaegi or otherwise. This is permanent, not deferred.
2. **Mirroring the sloth-common-sli-plugins catalog or its ids.** Recommended
   position (open question 1 below): slokit built-ins use a `slokit/` id
   namespace. Reusing `sloth-common/...` ids would imply option-for-option
   behavioral compatibility that slokit cannot promise without re-implementing
   and permanently tracking each Go plugin. A spec written for sloth's plugin
   catalog therefore fails with a clear "unknown plugin id" error rather than
   silently generating different rules. The README should state this
   explicitly.
3. **External plugin files in 0.9.** Runtime-loaded plugin definitions (YAML
   templates, WASM, anything file-based) are out of scope for 0.9; see open
   question 2. The registry API is designed so a file-based loader can be
   added later without breaking anything (a loader just registers plugins).
4. A richer template language. The only template token remains `{{.window}}`.
5. Plugin-provided alerting windows, labels, or anything beyond the SLI.
   Plugins expand to an SLI, full stop.

## 4. Plugin definition model

### 4.1 Options considered

| Model | Pros | Cons |
|-------|------|------|
| A. Built-in Rust registry (compile-time) | Full Rust power, strong typing, zero new deps, testable like any code, embedders extend via trait | New plugins require a slokit release (for built-ins) or Rust code (for embedders) |
| B. External YAML template files (runtime) | Ops teams share plugins without Rust; closest to sloth's workflow | Needs a real option-interpolation template language plus an escaping story (PromQL injection), a file discovery scheme, and a CLI flag; large surface to design well |
| C. Both in 0.9 | Complete | Doubles the surface under review; the template language deserves its own design pass |

### 4.2 Recommendation: model A for 0.9

Ship the Rust-native registry in 0.9. Reasons:

- The trait and registry are the foundation either way; a future file-based
  loader is "parse file, build a struct that implements `SliPlugin`, register
  it". Nothing in model A blocks model B later.
- The hard design problem in model B is the template language: option
  interpolation into PromQL needs escaping rules to avoid generating broken or
  malicious queries, and inventing that under the same review as the registry
  muddies both. slokit's whole 0.8.0 posture is "never emit output that does
  not load"; a naive `{{.options.foo}}` substitution undermines it.
- slokit's primary audiences today are CLI users (who get built-ins) and Rust
  embedders (who get the trait). Neither needs file loading yet.

### 4.3 Template language

Unchanged: the only token is the existing `{{.window}}` (and its spaced
variant `{{ .window }}`), substituted by `sli::substitute_window` at
generation time. Plugins do option interpolation in Rust when building the
query strings, so no new template engine exists. A plugin's expanded queries
must contain the window token exactly like hand-written ones; the existing
validation check enforces this even for buggy embedder plugins, because
expansion happens before query validation (section 5.3).

### 4.4 Option declaration, typing, and validation

Each plugin declares its options statically:

```rust
/// The kind of value an option accepts. Values arrive as strings
/// (sloth-compatible); the kind controls how they are checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionKind {
    /// Any string, passed through (selector fragments, regexes, metric names).
    String,
    /// Must parse as a finite f64.
    Number,
    /// Must be "true" or "false".
    Bool,
    /// Must parse via `Window::parse` (e.g. "5m", "1h").
    Duration,
}

/// Declaration of one option a plugin accepts.
#[derive(Debug, Clone)]
pub struct OptionSpec {
    /// Option name as written in the spec.
    pub name: &'static str,
    /// How the value is checked before expansion.
    pub kind: OptionKind,
    /// Whether the spec must provide it.
    pub required: bool,
    /// Default applied when an optional option is absent.
    pub default: Option<&'static str>,
    /// One-line description for docs and error messages.
    pub help: &'static str,
}
```

The registry (not each plugin) enforces the contract before calling
`expand`: unknown id, missing required option, and a value failing its
declared kind are all reported to the caller; defaults are applied. A plugin's
`expand` can therefore trust that required options are present and typed
values parse. Option names not declared by the plugin are **not** an error;
they are surfaced as a lint (section 5.3), because generation still succeeds
and the extra key may be a forward-compatible spec shared across slokit
versions.

Options whose values feed PromQL selector fragments (like the worked
example's `selector`) reuse the exact checks 0.8.0 added for the latency SLI
selector: no braces, no leading or trailing comma, balanced double quotes.
Same philosophy, same helpers, same error text style.

## 5. Spec surface

### 5.1 YAML shape

```yaml
version: "prometheus/v1"
service: myservice
slos:
  - name: requests-availability
    objective: 99.9
    sli:
      plugin:
        id: slokit/availability/http-requests-total
        options:
          selector: job="api"
          error_code_regex: "5..|429"
```

- `sli.plugin.id` (string, required): the registry key.
- `sli.plugin.options` (map, optional, default empty): option values. To stay
  a superset of sloth's `map[string]string`, values deserialize from any YAML
  scalar (string, number, bool) and are coerced to strings by a small custom
  deserializer; `threshold: 0.5` and `threshold: "0.5"` are equivalent.
  Non-scalar values (maps, lists) are a parse error.

Rust side, `SliSpec` gains a fourth field:

```rust
pub struct SliSpec {
    pub events: Option<EventsSli>,
    pub raw: Option<RawSli>,
    pub latency: Option<LatencySli>,
    /// Plugin-provided SLI (sloth-compatible spec surface).
    pub plugin: Option<PluginSli>,
}

pub struct PluginSli {
    pub id: String,
    pub options: BTreeMap<String, String>,
}
```

### 5.2 Resolution order versus events/raw/latency

There is no precedence: exactly one of `events`, `raw`, `latency`, `plugin`
must be set, extending the existing mutual-exclusion check in
`SloSpec::to_sli` from three shapes to four. Setting `plugin` together with
any other shape is the same "sets multiple SLIs" validation error users
already get today, and setting none extends the existing "has no ... SLI"
error message to name `plugin` too. Built-in shapes are never shadowed:
plugin ids live in their own registry namespace and cannot collide with the
literal keys `events`, `raw`, or `latency`.

Expansion happens inside `to_sli` resolution: a `plugin` SLI is resolved
against a registry and becomes an ordinary core `Sli` (usually
`Sli::Events` or `Sli::Raw`) before validation and generation ever see it.
Downstream code (recording rules, alerts, dashboard, check) is unchanged and
unaware plugins exist.

### 5.3 Errors and lints

Hard validation errors (output impossible or broken, per the 0.8.0
philosophy):

- `sli.plugin.id` empty or whitespace-only.
- Unknown plugin id: no query can be generated. The message names the id and
  points at the registry, e.g.
  `slo 'a': unknown SLI plugin 'slokit/availabilty/http' (not in the plugin registry)`.
- Missing required option, named explicitly with the plugin's `help` text.
- Option value failing its declared kind (e.g. `OptionKind::Number` given
  `"abc"`).
- Selector-shaped option values failing the 0.8.0 selector checks (braces,
  stray commas, unbalanced quotes).
- Post-expansion: the expanded SLI's queries flow through the existing
  window-token validation, so an embedder plugin that emits a query without
  `{{.window}}` is caught exactly like a hand-written spec would be.

New lint (advisory; output loads fine):

- `PLUGIN_UNKNOWN_OPTION` (warning): an option name the plugin does not
  declare. Likely a typo of a real option, but generation succeeds, so it
  follows `SPEC_VERSION` and friends into lint rather than validation.

No other lint changes are needed: existing lints (`THRESHOLD_UNREACHABLE`,
`PERIOD_TOO_SHORT`, alert-label checks) operate on objective, period, and
alerting, which plugins do not touch.

## 6. API surface

### 6.1 Trait and registry

New module `src/spec/plugin.rs`, exported as `slokit::spec::plugin`:

```rust
/// A reusable, named SLI template: expands declared options into a core Sli.
pub trait SliPlugin: Send + Sync {
    /// Stable identifier referenced by `sli.plugin.id`.
    fn id(&self) -> &str;
    /// One-line description for docs and future listing commands.
    fn description(&self) -> &str;
    /// The options this plugin accepts; the registry validates against these
    /// before calling `expand`.
    fn options(&self) -> &[OptionSpec];
    /// Build the SLI. Called with defaults applied and declared kinds already
    /// checked; `expand` only needs plugin-specific semantic checks.
    fn expand(&self, options: &BTreeMap<String, String>) -> Result<Sli>;
}

/// Holds plugins by id. `Default` is `with_builtins()`.
pub struct SliPluginRegistry { /* BTreeMap<String, Box<dyn SliPlugin>> */ }

impl SliPluginRegistry {
    /// A registry with no plugins at all.
    pub fn empty() -> Self;
    /// A registry preloaded with slokit's built-in plugins.
    pub fn with_builtins() -> Self;
    /// Add a plugin. Errors on a duplicate id (no silent shadowing,
    /// including of built-ins).
    pub fn register(&mut self, plugin: Box<dyn SliPlugin>) -> Result<()>;
    /// Look up a plugin by id.
    pub fn get(&self, id: &str) -> Option<&dyn SliPlugin>;
    /// Registered ids, sorted (stable output for docs and errors).
    pub fn ids(&self) -> impl Iterator<Item = &str>;
    /// Full resolution: look up `id`, apply defaults, check required options
    /// and kinds, then expand. This is what spec resolution calls.
    pub fn resolve(&self, id: &str, options: &BTreeMap<String, String>) -> Result<Sli>;
}
```

Design notes:

- `register` refusing duplicate ids keeps behavior predictable: an embedder
  cannot accidentally (or deliberately) redefine what a built-in id generates
  for specs shared across teams. If genuine overriding is ever wanted, an
  explicit `register_override` can be added later without breaking anything.
- Errors use a new `SlokitError::Plugin(String)` variant for registry-level
  failures (duplicate id, unknown id, bad options). Adding a variant to the
  public enum is an additive pre-1.0 change, consistent with the CHANGELOG
  policy, and v0.12.0 plans `#[non_exhaustive]` anyway. During spec
  validation these surface inside the usual aggregated
  `SlokitError::Validation` lines, exactly like `to_sli` errors do today.

### 6.2 Threading through validate, lint, and generate

Existing entry points keep their signatures and default to the built-in
registry; each gains a `_with` sibling for embedders with custom plugins:

- `SloSpec::to_sli()` resolves `plugin` SLIs against
  `SliPluginRegistry::with_builtins()`; new
  `SloSpec::to_sli_with(&SliPluginRegistry)`.
- `Spec::validate()` / `spec::validate_all` unchanged (builtins); new
  `validate_with` / `validate_all_with`.
- `Spec::lint()` unchanged; new `lint_with` (needed so
  `PLUGIN_UNKNOWN_OPTION` can compare option names against the right
  declarations).
- `GenerateOptions` gains `plugins: std::sync::Arc<SliPluginRegistry>`
  (default: builtins). `Arc` because `GenerateOptions` is `Clone` and a
  boxed-trait registry is not; `Debug` is hand-implemented to print the ids.
  This is a struct-literal-breaking field addition with the same
  `..Default::default()` mitigation as `period_aware` in 0.7.0. (Alternative
  considered: parallel `generate_rules_with_plugins` functions; rejected as
  API sprawl since every consumer would need the plugin-aware variants
  eventually. See open question 4.)

Consequence worth stating plainly: a spec that references an
embedder-registered plugin fails plain `Spec::validate()` with "unknown SLI
plugin". That is correct behavior, not a bug: without the embedder's registry
there is genuinely no way to generate output for it. The CLI in 0.9 always
uses the built-in registry.

### 6.3 Feature placement and the lean core

Everything in this design lives behind the existing `spec` feature:
`src/spec/plugin.rs` plus the `SliSpec::plugin` field. No new crates, no new
feature flags. The registry is a `BTreeMap` of boxed trait objects; option
checking reuses `Window::parse` and `f64::parse` from core.

The lean core contract is unchanged and verifiable: `cargo build
--no-default-features` compiles exactly the code it does today (the existing
lean-core CI job guards this). The `SliPlugin` trait is deliberately **not**
placed in core: its only consumer is spec resolution, and core users who want
a computed SLI can already construct `Sli` values directly.

## 7. Worked example: `slokit/availability/http-requests-total`

The first built-in, chosen because it is the copy-pasted query this feature
exists to kill.

### Definition (in slokit, compiled in)

- id: `slokit/availability/http-requests-total`
- description: availability from an `http_requests_total`-style counter,
  where responses matching an error-code regex are the bad events
- options:

| name | kind | required | default | meaning |
|------|------|----------|---------|---------|
| `metric` | String | no | `http_requests_total` | counter metric name (validated with the 0.8.0 metric-name check, since it is embedded unquoted) |
| `selector` | String | no | (none) | label matchers without braces, e.g. `job="api"` (0.8.0 selector checks apply) |
| `error_code_regex` | String | no | `5..` | regex for the `code` label identifying bad events |

- expansion: an `Sli::Events` with

  ```text
  error_query: sum(rate(http_requests_total{job="api", code=~"5.."}[{{.window}}]))
  total_query: sum(rate(http_requests_total{job="api"}[{{.window}}]))
  ```

  (without `selector`, the matchers are `{code=~"5.."}` and none.)

### Spec usage

```yaml
version: "prometheus/v1"
service: myservice
labels:
  owner: team-platform
slos:
  - name: requests-availability
    objective: 99.9
    description: "99.9% of requests succeed"
    sli:
      plugin:
        id: slokit/availability/http-requests-total
        options:
          selector: job="api"
    alerting:
      page_alert:
        labels: { severity: page }
      ticket_alert:
        labels: { severity: ticket }
```

### Generated output (excerpt)

The SLI recording group carries one rule per effective MWMBR window plus the
period; the 5m rule:

```yaml
groups:
  - name: slokit-slo-sli-recordings-myservice-requests-availability
    rules:
      - record: slo:sli_error:ratio_rate5m
        expr: |-
          (sum(rate(http_requests_total{job="api", code=~"5.."}[5m])))
          /
          (sum(rate(http_requests_total{job="api"}[5m])))
        labels:
          owner: team-platform
          sloth_id: myservice-requests-availability
          sloth_service: myservice
          sloth_slo: requests-availability
          sloth_window: 5m
```

Metadata and alert groups are byte-identical to what the equivalent
hand-written `events` spec produces today, because after expansion the
generator sees an ordinary `Sli::Events`. That byte-identity is a golden-test
invariant, and the fixture joins the existing `promtool check rules` test set
(with the CI job that sets `SLOKIT_REQUIRE_PROMTOOL=1`), so the plugin path is
externally validated end to end.

## 8. Implementation plan for v0.9.0 (PR 2, blocked on this doc)

### 8.1 Slices

Two small PRs, matching the roadmap sizing:

**PR 2 (the engine):**

1. `src/spec/plugin.rs`: `OptionKind`, `OptionSpec`, `SliPlugin`,
   `SliPluginRegistry` with `resolve`, `SlokitError::Plugin` variant.
2. `SliSpec::plugin` field with the scalar-coercing options deserializer;
   `to_sli` mutual exclusion extended to four shapes; `to_sli_with`.
3. `validate`/`validate_all`/`lint` awareness plus the `_with` siblings and
   the `PLUGIN_UNKNOWN_OPTION` lint.
4. `GenerateOptions.plugins` threading.
5. The `slokit/availability/http-requests-total` built-in.
6. Tests per 8.2, README section (including the sloth-compatibility honesty
   paragraph from section 3), CHANGELOG entry.

**PR 3 (small, polish; can fold into PR 2 if it stays small):**

1. A second built-in to prove the registry generalizes, e.g.
   `slokit/availability/grpc-server-handled` (bad events =
   `grpc_server_handled_total` with `grpc_code` outside an allowlist).
2. Rustdoc examples for embedder registration (custom plugin + `_with` flow).
3. Error-message polish: unknown-id message lists registered ids when the
   registry is small.

Then the 0.9.0 release cut (USER-ONLY, per roadmap policy).

### 8.2 Test strategy

- **Unit**: registry register/duplicate/get/ids; `resolve` paths (unknown id,
  missing required, kind failures for Number/Bool/Duration, defaults
  applied); built-in expansion with and without `selector`, custom `metric`,
  custom `error_code_regex`; selector and metric-name rejection reusing the
  0.8.0 checks; mutual-exclusion errors for every pair including `plugin`;
  options deserializer scalar coercion and non-scalar rejection.
- **Golden**: insta snapshot of the full generated rules YAML for the worked
  example, plus an equality assertion that the plugin spec and its
  hand-written `events` twin generate byte-identical `RuleSet`s.
- **promtool**: the plugin fixture is added to the existing
  `promtool check rules` tests and the CI job that requires promtool.
- **Embedder path**: a test-local toy plugin registered on an `empty()`
  registry, exercised through `to_sli_with`, `validate_with`, and generation;
  includes a deliberately broken toy plugin (no window token) proving
  post-expansion validation catches it.
- **Lint**: `PLUGIN_UNKNOWN_OPTION` fires for undeclared names and stays
  quiet for declared ones.

### 8.3 Out of scope for 0.9

- External plugin files of any kind (YAML templates, WASM, dylibs) and any
  `--sli-plugins-path`-style CLI flag.
- Executing sloth Go plugins (permanent non-goal, restated for clarity).
- A `slokit plugins` listing subcommand (natural once external files exist;
  candidate for the "Later" roadmap bucket).
- Mirroring sloth-common plugin ids or porting its catalog.
- Plugin-provided alerting windows, labels, or dashboard content.
- OpenSLO interaction (v0.10.0 owns the OpenSLO funnel; whether OpenSLO
  input can reference slokit plugin ids is decided there).

## 9. Open questions (maintainer decisions needed)

These are genuine judgment calls; each has a recommendation but changes the
public surface, so they need an explicit call before PR 2.

1. **Id namespace.** Use `slokit/...` ids for built-ins (recommended), or
   additionally mirror selected `sloth-common/...` ids for drop-in spec
   portability? Mirroring means promising behavioral equivalence with Go
   plugins slokit does not run, and tracking upstream changes forever.
2. **External plugin files.** Confirm deferring runtime-loaded plugin
   definitions past 0.9 (recommended), or require them in 0.9? If deferred,
   should the roadmap's "Later" bucket gain an explicit entry so the intent
   is recorded?
3. **Built-in set for 0.9.** Ship two built-ins (http availability + grpc
   availability, recommended) or just the one worked example? A latency
   plugin is deliberately absent: `sli.latency` already covers it natively.
4. **Registry threading.** Accept the `GenerateOptions.plugins:
   Arc<SliPluginRegistry>` field with its one-time struct-literal breakage
   (recommended, precedented by `period_aware` in 0.7.0), or keep
   `GenerateOptions` untouched and add parallel `*_with_plugins` generate
   functions?
5. **Unknown option names.** Lint warning `PLUGIN_UNKNOWN_OPTION`
   (recommended, consistent with the 0.8.0 "output still loads" bar), or a
   hard validation error (stricter, catches typos of optional options at the
   cost of rejecting forward-compatible specs)?
