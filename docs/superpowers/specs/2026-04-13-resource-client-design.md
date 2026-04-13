# ResourceClient Trait and k8s REST Backend — Design

## Context

Flotilla's convoy system needs a k8s-style resource API (get/list/watch/create/update/delete with resourceVersion). Controllers are written against a trait so they can run against k8s REST (prototyping and power users), a future flotilla-cp HTTP backend, or an in-process resource server (zero-dependency laptop case). See `docs/superpowers/specs/2026-04-13-convoy-and-control-plane-design.md` for the full vision.

This design covers stage 1 of the convoy implementation: define the `ResourceClient` trait and implement it against real k8s via raw REST calls (reqwest + serde, not kube-rs). A second in-memory implementation provides a test double for controller tests in later stages.

## Crate

Single new crate: `crates/flotilla-resources`. Lives in the flotilla workspace from the start — this is permanent code, not a throwaway prototype. The k8s backend can move behind a feature flag later when other crates depend only on the trait.

## Resource Trait and Types

### ApiPaths

Packages the k8s API coordinates for a resource type:

```rust
struct ApiPaths {
    pub group: &'static str,   // e.g. "flotilla.io"
    pub version: &'static str, // e.g. "v1"
    pub plural: &'static str,  // e.g. "convoys"
    pub kind: &'static str,    // e.g. "Convoy"
}
```

### Resource Trait

A Rust type representing a k8s-style resource. Carries its own API coordinates and associated spec/status types:

```rust
trait Resource: Serialize + DeserializeOwned + Send + Sync + 'static {
    type Spec: Serialize + DeserializeOwned + Send + Sync;
    type Status: Serialize + DeserializeOwned + Send + Sync;

    const API_PATHS: ApiPaths;
}
```

### ResourceObject

The k8s-style resource envelope. Every resource has metadata, a spec (desired state), and an optional status (observed state, written by controllers):

```rust
struct ResourceObject<T: Resource> {
    pub metadata: ObjectMeta,
    pub spec: T::Spec,
    pub status: Option<T::Status>,
}
```

### ObjectMeta

Standard resource metadata, compatible with k8s conventions:

```rust
struct ObjectMeta {
    pub name: String,
    pub namespace: String,
    pub resource_version: Option<String>,
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
    pub creation_timestamp: Option<DateTime<Utc>>,
}
```

Fields included: name, namespace, resourceVersion (optimistic concurrency), labels (selection/filtering), annotations (arbitrary metadata), creationTimestamp (display/debugging).

Fields deferred: generation/observedGeneration, finalizers, ownerReferences.

## ResourceClient and ResourceResolver

### ResourceClient

A factory trait that produces resolvers. Two paths:

```rust
trait ResourceClient: Send + Sync {
    fn using<T: Resource>(&self) -> ResourceResolver<T>;
    fn paths(&self, paths: &ApiPaths) -> ResourceResolver<serde_json::Value>;
}
```

- `using::<T>()` — typed path. `T: Resource` provides API coordinates and serde types. This is the normal path for controllers.
- `paths()` — escape hatch. Caller supplies API coordinates, gets back `Value`. For operating on resources whose Rust type isn't known at compile time.

### ResourceResolver

The thing with actual CRUD + watch methods. Holds an `Arc` to the backend (needed for async watch streams that outlive the borrow):

```rust
impl<T: Resource> ResourceResolver<T> {
    async fn get(&self, name: &str) -> Result<ResourceObject<T>, ResourceError>;
    async fn list(&self) -> Result<Vec<ResourceObject<T>>, ResourceError>;
    async fn create(&self, obj: &ResourceObject<T>) -> Result<ResourceObject<T>, ResourceError>;
    async fn update(&self, obj: &ResourceObject<T>) -> Result<ResourceObject<T>, ResourceError>;
    async fn update_status(&self, obj: &ResourceObject<T>) -> Result<ResourceObject<T>, ResourceError>;
    async fn delete(&self, name: &str) -> Result<(), ResourceError>;
    async fn watch(&self) -> Result<impl Stream<Item = Result<WatchEvent<T>, ResourceError>>, ResourceError>;
}
```

The `paths()` escape hatch returns `ResourceResolver<serde_json::Value>`, which has the same methods but operates on raw JSON values instead of typed `ResourceObject<T>`. The exact API shape of the `Value` resolver will be determined by usage — it may return `Value` directly rather than `ResourceObject<Value>`.

- `update` and `update_status` are separate — spec and status are written by different actors (user writes spec, controller writes status). Mirrors k8s's `/status` subresource.
- `create` returns the created object with server-assigned resourceVersion and creationTimestamp.
- `update` requires the object to carry a resourceVersion. Stale version returns `ResourceError::Conflict`.
- `watch` returns a stream — outer `Result` for connection failure, inner `Result` per event for parse/stream errors.

## Watch and Error Types

### WatchEvent

```rust
enum WatchEvent<T> {
    Added(ResourceObject<T>),
    Modified(ResourceObject<T>),
    Deleted(ResourceObject<T>),
}
```

### ResourceError

```rust
enum ResourceError {
    NotFound { name: String },
    Conflict { name: String, message: String },
    Invalid { message: String },
    Unauthorized { message: String },
    Other { message: String },
}
```

`NotFound` and `Conflict` are the two that controllers branch on constantly. `Invalid` covers schema/validation rejection. `Unauthorized` for auth problems. `Other` is the catch-all for transport errors, unexpected status codes, deserialization failures. More variants can be added as controllers need them.

## k8s REST Backend

### K8sClient

```rust
struct K8sClient {
    http: reqwest::Client,
    base_url: String,       // e.g. "https://192.168.49.2:8443"
    namespace: String,       // default: "flotilla"
}
```

### URL Construction

ResourceResolver builds URLs from ApiPaths + namespace:

| Operation | URL pattern |
|-----------|-------------|
| get, update, delete | `/apis/{group}/{version}/namespaces/{namespace}/{plural}/{name}` |
| list, create | `/apis/{group}/{version}/namespaces/{namespace}/{plural}` |
| update_status | `/apis/{group}/{version}/namespaces/{namespace}/{plural}/{name}/status` |
| watch | list URL with `?watch=true&resourceVersion=...` |

### Authentication

For minikube: parse `~/.kube/config` to get server URL and client certificate/key. Configure reqwest with the client certificate identity. No token management needed.

### Watch Implementation

HTTP GET with `?watch=true`, chunked transfer encoding. Read newline-delimited JSON objects from the response body stream, deserialize each as `WatchEvent<T>`. The stream stays open until the server closes it or the caller drops it.

### CRD Registration

On startup, PUT the hand-written CRD YAML (embedded via `include_str!`) to `/apis/apiextensions.k8s.io/v1/customresourcedefinitions/{name}`. PUT is idempotent — works for both create and update. Also create the namespace if missing (POST to `/api/v1/namespaces`).

CRD specs are hand-written YAML, not generated from Rust macros. Stored in `src/crds/` within the crate.

## In-Memory Backend

A test double that mirrors real k8s semantics without a running cluster. Used by controller tests in stages 2-3.

### Storage

```rust
struct InMemoryClient {
    stores: Arc<Mutex<HashMap<StoreKey, ResourceStore>>>,
}

// Keyed by (group, version, plural, namespace)
type StoreKey = (String, String, String, String);

struct ResourceStore {
    objects: HashMap<String, String>,          // name -> JSON string
    next_version: u64,
    watchers: Vec<mpsc::Sender<(u64, String)>>, // version + event JSON
    event_log: Vec<(u64, String)>,             // for watch replay
}
```

### Behaviors

- **resourceVersion**: monotonic counter per resource type, incremented on every mutation. Returned on create/update, required on update.
- **Conflict detection**: update with stale resourceVersion returns `ResourceError::Conflict`.
- **Watch**: mutations push events to registered watcher channels. Watch with a resourceVersion replays events from the event log since that version.
- **NotFound**: get/update/delete on missing name returns `ResourceError::NotFound`.
- **Namespace support**: namespace is part of the store key, same as the k8s backend.

This is a test double, not a production store. No persistence, no label filtering on list (add when controllers need it).

## Crate Structure

```
crates/flotilla-resources/
├── Cargo.toml
├── src/
│   ├── lib.rs              -- re-exports
│   ├── resource.rs          -- Resource trait, ApiPaths, ResourceObject, ObjectMeta
│   ├── client.rs            -- ResourceClient trait, ResourceResolver
│   ├── error.rs             -- ResourceError
│   ├── watch.rs             -- WatchEvent
│   ├── k8s/
│   │   ├── mod.rs           -- K8sClient
│   │   ├── kubeconfig.rs    -- ~/.kube/config parsing, auth
│   │   └── crd.rs           -- CRD registration
│   ├── in_memory.rs         -- InMemoryClient
│   └── crds/
│       └── convoy.crd.yaml  -- hand-written CRD, embedded via include_str!
├── examples/
│   └── k8s_crud.rs          -- demo binary against minikube
└── tests/
    └── k8s_integration.rs   -- integration tests against minikube
```

### Dependencies

All already in the workspace except `futures` for the `Stream` trait:

- `serde`, `serde_json` — serialization
- `reqwest` with `rustls-tls` — HTTP client for k8s backend
- `tokio` — async runtime, `mpsc` channels for watch
- `chrono` — timestamps
- `serde_yml` — CRD YAML parsing for registration
- `futures` — `Stream` trait for watch

## Deliverables

Stage 1 produces:

1. The `Resource` trait, `ResourceClient` trait, and associated types
2. `K8sClient` implementation — CRUD + watch against minikube via raw REST
3. `InMemoryClient` implementation — test double with resourceVersion, conflict detection, watch
4. Hand-written convoy CRD YAML
5. Example binary that registers CRDs, exercises CRUD + watch against minikube
6. Integration tests against minikube

No controller logic. Stages 2-3 (WorkflowTemplate, Convoy controller) are the first real consumers of the trait.

## Design Decisions

### Raw REST over kube-rs

kube-rs is a heavy dependency with opinions about async runtime. The `ResourceClient` trait reflects plain REST semantics, not kube-rs's `Api<T>` abstractions. For prototyping, a simple watch-and-react loop is clearer than kube-rs's reconciler framework.

### Hand-written CRD YAML

CRD diffs are readable. Macro-generated CRD output is opaque. k8s CRD specs have `x-kubernetes-*` annotations and structural schema requirements that fight macro generation. Hand-written YAML is debuggable with `kubectl apply --dry-run`.

### Typed over dynamic

The primary path is fully generic (`T: Resource`). Controllers know their types at compile time and get full serde type safety. The `paths()` escape hatch to `Value` exists but doesn't drive the design.

### Separate spec/status update paths

Mirrors k8s's status subresource convention. Users/CLI write spec, controllers write status. Separate update methods prevent accidental cross-contamination and reduce conflict surface.
