# PDT — Platform Data Toolkit

A document/asset store API with relationship graphs, free-form tagging, full-text search,
OIDC authentication, and Cedar-based authorization. Runs as a single global instance or as
a tree of isolated per-tenant instances, each backed by its own SQLite database.

## Features

- **Assets, relations, collections** — CRUD with soft deletes, a traversable relation
  graph, and named collections
- **Free-form tags** — `{category, value}` pairs with light validation; no fixed taxonomy
- **Full-text search** — SQLite FTS5 with user input safely reduced to quoted phrases
- **Multi-instance tenancy** — `X-Instance-Id` routing to per-instance SQLite files,
  nested provisioning (workspace → agent/company leaves), instance tree listing
- **Two storage backends** — MongoDB/DocumentDB (default) or SQLite for the root database
- **OIDC auth + Cedar authorization** — JWT validation via a policy enforcement point
  ([pep](https://crates.io/crates/pep)), Cedar policies enforced per request, audit log
  with user attribution
- **OpenAPI** — machine-readable spec at `/api/docs/openapi.json`

## Quick start

Zero external dependencies with the SQLite root backend:

```bash
cargo run --features sqlite-backend --bin pdt
# …or let it read a .env file (dotenvy is loaded automatically):
cp env.example .env
```

Environment for the above:

```bash
PDT_DB_BACKEND=sqlite
SQLITE_PATH=./pdt.db
PDT_INSTANCES_DIR=./instances
AUTH_ENABLED=false
AUTH_DEV_MODE=true
```

To run against MongoDB/DocumentDB instead (the default build):

```bash
DOCUMENTDB_URL=mongodb://localhost:27017
DOCUMENTDB_USERNAME=admin
DOCUMENTDB_PASSWORD=change-me
DOCUMENTDB_DATABASE=pdt
```

See `env.example` for the full set of variables.

## Multi-instance tenancy

Every request may carry `X-Instance-Id`. With the header, all entity operations are routed
to that instance's own SQLite database under `PDT_INSTANCES_DIR`; without it, they hit the
root database (MongoDB/DocumentDB by default, SQLite with the `sqlite-backend` feature).

```
POST /api/instances/{id}/provision?parent={parent_id}   # create an instance DB (optionally nested)
GET  /api/instances                                     # list instances found on disk
GET  /api/instances/tree                                # list as a workspace tree
DELETE /api/instances/{id}                              # delete an instance
```

Nested provisioning creates `{instances_dir}/{parent}/{id}/pdt.db`, which gives you
workspace → agent/company hierarchies where each leaf is fully isolated: cross-instance
reads simply don't resolve. Assets are born with an `auth_context`
(visibility / owner groups / confidentiality) that Cedar policies evaluate.

## Authentication and authorization

Write endpoints (and sensitive reads) require `Authorization: Bearer <jwt>`. Tokens are
validated against the configured OIDC issuer (JWKS signature, expiry, audience):

```bash
AUTH_ENABLED=true
AUTH_ISSUER_URL=https://idp.example.com
AUTH_AUDIENCE=pdt-api
AUTH_USERINFO_URL=https://idp.example.com/userinfo   # optional claim enrichment
```

Authorization is delegated to [Cedar](https://www.cedarpolicy.com/). Policies live in
`policies/` (`rbac.cedar` + `schema.cedarschema`), are compiled into the binary, and the
effective policy set is exposed at `GET /api/cedar/policies`.

> ⚠️ **`AUTH_DEV_MODE=true` injects admin claims into any request that carries no bearer
> token — including when `AUTH_ENABLED=true`.** Never enable it outside local development.

## API surface

```
# Assets
POST   /api/assets                          GET    /api/assets
GET    /api/assets/{id}                     PUT    /api/assets/{id}
DELETE /api/assets/{id}                     # soft delete
POST   /api/assets/{id}/tags                DELETE /api/assets/{id}/tags/{tag_id}
PUT    /api/assets/{id}/auth-context        # update auth context
GET    /api/assets/{id}/relations           GET    /api/assets/{id}/graph
GET    /api/assets/{id}/history

# Relations
POST   /api/relations                       GET/DELETE /api/relations/{id}

# Collections
POST   /api/collections                     GET    /api/collections
GET/PUT/DELETE /api/collections/{id}
POST   /api/collections/{id}/assets         DELETE /api/collections/{id}/assets/{asset_id}

# Search & audit
GET    /api/search?q=…                      GET    /api/audit

# Instances
POST   /api/instances/{id}/provision        DELETE /api/instances/{id}
GET    /api/instances                       GET    /api/instances/tree

# Misc
GET    /health                              GET    /api/cedar/policies
```

### Search

`GET /api/search?q=fame workspace` — queries are tokenized and each token is phrase-quoted
before hitting FTS5, so user input can never be interpreted as FTS query syntax
(column filters, boolean operators, etc.). Non-alphanumeric characters are preserved
inside quoted phrases, making e.g. Persian text searchable.

### Tags

Tags are free-form `{category, value}` pairs — categories like `asset_type` are
conventions, not enforced values. Validation: category/value are alphanumeric plus
hyphens, underscores, and forward slashes, capped at 64 characters.

### Relation types

`contains`, `references`, `related_to`, `depends_on`, `supersedes`, `complements`,
`has_instance`, `has_agent`

## Development

```bash
cargo test                            # default (mongodb-backend) suite
cargo test --features sqlite-backend  # sqlite root-backend suite
```

Two binaries are produced by the workspace; `--bin pdt` selects the server. The
`sqlite-backend` feature gates only the **root** database repository — instance routing
always uses SQLite, so `sqlx` is compiled in unconditionally.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
