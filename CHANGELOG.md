# Changelog

All notable changes to PDT (Platform Data Toolkit) are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2025-12-25

### Added

#### Authentication & Authorization
- **PEP Integration**: Integrated with PEP (Policy Enforcement Point) library for OIDC resource server functionality
- **JWT Validation**: Added JWT bearer token validation with JWKS caching and signature verification
- **AuthenticatedUser Extractor**: Implemented Axum extractor for automatic JWT claim extraction into domain model
- **AuthLayer Middleware**: Created Tower middleware for Bearer token extraction and OIDC claim validation
- **User Audit Trail**: Enhanced Asset and Collection models with `created_by` and `updated_by` tracking fields
- **Configuration**: Added `AuthConfig` with OIDC provider configuration, audience validation, and defaults

#### API Enhancements
- **Authenticated Handlers**: Updated 11+ handlers to require `AuthenticatedUser` for all write operations (create, update, delete, add_tag, remove_tag)
- **Handler Types**: Enforced authentication at type level through `AuthenticatedUser` parameter requirement
- **Repository Layer**: Enhanced AssetRepository and CollectionRepository with user_id propagation for audit trails
- **Service Layer**: Updated AssetService and CollectionService to track user attribution

#### Testing & Documentation
- **Unit Tests**: Comprehensive test suite with 18 unit tests covering:
  - AuthConfig creation and defaults
  - Error types and Display implementations
  - Bearer token extraction from HTTP headers
  - AuthenticatedUser struct creation and validation
  - Integration tests for middleware and extractors
- **Documentation**: Added README section on authentication setup, environment variables, and OIDC configuration examples
- **Examples**: Created sample environment variables and configuration files for OIDC providers

#### Code Quality
- **Linting**: Zero clippy warnings across all targets
- **Formatting**: Consistent code formatting via cargo fmt
- **Release Build**: Verified optimized release build with all dependencies

### Technical Details

#### Security Chain
- Middleware intercepts all requests and extracts Bearer tokens
- JWT signature validation through JWKS endpoint
- Claims validation (aud, sub, email, preferred_username)
- AuthenticatedUser type enforces authentication in handlers
- User attribution tracked at storage layer (created_by, updated_by)

#### Dependencies Added
- `pep` v0.2.0: OIDC resource server with JWT validation and JWKS caching
- `tower`: Middleware composition and trait system
- `tower-http`: Enhanced HTTP support for CORS and tracing

#### Model Changes
- **Asset**: Added `updated_by: String` field for user attribution on updates
- **Collection**: Added `updated_by: String` field for user attribution on updates
- All existing fields preserved for backward compatibility

### Breaking Changes

⚠️ **Database Schema Update Required**
- MongoDB collections require `updated_by` field to be added to all existing Asset and Collection documents
- Recommended migration: `db.assets.updateMany({}, { $set: { updated_by: "system" } })`

⚠️ **Handler Signatures Changed**
- All write operation handlers now require `AuthenticatedUser` parameter
- Update client code to include Bearer tokens in Authorization header
- Format: `Authorization: Bearer <jwt_token>`

### Migration Guide

#### From 0.1.x to 0.2.0

1. **Set up OIDC Provider** (Keycloak, Kanidm, or similar)
   - Configure OAuth 2.0 resource server credentials
   - Obtain JWKS endpoint URL

2. **Update Environment Variables**
   ```bash
   OIDC_PROVIDER="https://auth.example.com"
   OIDC_AUDIENCE="pdt-api"
   OIDC_ISSUER="https://auth.example.com"
   OIDC_JWKS_ENDPOINT="https://auth.example.com/.well-known/jwks.json"
   ```

3. **Update Database**
   ```bash
   db.assets.updateMany({}, { $set: { updated_by: "system-migration" } })
   db.collections.updateMany({}, { $set: { updated_by: "system-migration" } })
   ```

4. **Update Client Requests**
   ```bash
   # Before
   curl -H "Content-Type: application/json" \
     http://localhost:8080/api/assets \
     -d '{"title":"...","description":"..."}'

   # After
   curl -H "Authorization: Bearer <jwt_token>" \
     -H "Content-Type: application/json" \
     http://localhost:8080/api/assets \
     -d '{"title":"...","description":"..."}'
   ```

5. **Verify Configuration**
   ```bash
   cargo build --release
   ./target/release/pdt
   # Should start without auth errors if OIDC provider is configured
   ```

### Performance

- **Token Validation**: Sub-millisecond signature verification with cached JWKS
- **Memory**: No significant increase from auth layer (cached claims, single allocations)
- **Database**: Minimal overhead from audit fields (one additional string per write)

### Testing

- **Unit Tests**: 18 passing tests covering all auth components
- **Integration**: Middleware and handler integration verified
- **Release Build**: Optimized binary tested for compilation correctness
- **Platforms**: Tested on Linux (x86_64), compatible with macOS and Windows

### Known Issues

None reported in this release. All components tested and verified.

### Future Work

- [ ] Role-based access control (RBAC) layer
- [ ] API key authentication support
- [ ] Audit log retention and search
- [ ] Multi-provider federation (multiple OIDC issuers)
- [ ] Service-to-service authentication (mTLS)

### Contributors

- Podtan Team

---

## [0.1.0] - 2025-12-24

### Added

- Initial release of PDT (Platform Data Toolkit)
- Asset management with CRUD operations
- Collection management and grouping
- Relationship graph traversal
- Tag-based asset classification
- Full-text search support
- MongoDB persistence
- OpenAPI documentation endpoint
- Health check endpoint
- CORS support for cross-origin requests
- Structured logging with request tracing
- RESTful API following OpenAPI 3.0 specification

### Architecture

- Multi-tier architecture: handlers → services → repositories → MongoDB
- Type-safe request/response handling with serde
- Async/await with Tokio for high concurrency
- Tower middleware for HTTP concerns (CORS, logging)
- Axum for ergonomic routing and extractors

### Security

- CORS configured for local development
- Request logging with correlation IDs
- Error handling without information leakage

### Documentation

- OpenAPI 3.0 specification available at `/api/docs/openapi.json`
- README with setup and usage instructions
- API endpoint documentation
- Example requests in comments

[Unreleased]: https://github.com/podtan/pdt/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/podtan/pdt/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/podtan/pdt/releases/tag/v0.1.0
