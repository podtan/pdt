# Platform Data Toolkit (PDT)

PDT is a centralized API server that serves as an enterprise knowledge silo for company-specific concepts, documents, and their relationships.

## Features

- **Knowledge Asset Management**: Store and manage knowledge assets with rich metadata
- **Tag-Based Classification**: Multi-dimensional tagging including asset type, business domain, language, etc.
- **Graph-Based Relationships**: Define and traverse relationships between assets
- **Concept Collections**: Organize assets into named collections
- **Search & Discovery**: Full-text search and tag-based filtering
- **Audit Logging**: Track all changes with user attribution

## Quick Start

### Prerequisites

- Rust 1.70+
- DocumentDB/MongoDB instance

### Configuration

Create a `.env` file:

```bash
# DocumentDB/MongoDB Storage Backend
DOCUMENTDB_URL=mongodb://localhost:10260
DOCUMENTDB_USERNAME=trustee
DOCUMENTDB_PASSWORD=abk12345
DOCUMENTDB_DATABASE=pdt
DOCUMENTDB_TLS=true
DOCUMENTDB_TLS_ALLOW_INVALID=true

# Server configuration (optional)
PDT_HOST=0.0.0.0
PDT_PORT=8080
```

### Build and Run

```bash
# Build
cargo build --release

# Run
cargo run --release
```

## API Endpoints

### Assets

- `POST /api/assets` - Create asset
- `GET /api/assets` - List assets
- `GET /api/assets/:id` - Get asset by ID
- `PUT /api/assets/:id` - Update asset
- `DELETE /api/assets/:id` - Delete asset (soft delete)
- `POST /api/assets/:id/tags` - Add tag to asset
- `DELETE /api/assets/:id/tags/:tag_id` - Remove tag from asset

### Relations

- `POST /api/relations` - Create relation
- `GET /api/relations/:id` - Get relation by ID
- `DELETE /api/relations/:id` - Delete relation
- `GET /api/assets/:id/relations` - Get all relations for an asset
- `GET /api/assets/:id/graph` - Traverse relationship graph

### Collections

- `POST /api/collections` - Create collection
- `GET /api/collections` - List collections
- `GET /api/collections/:id` - Get collection by ID
- `PUT /api/collections/:id` - Update collection
- `DELETE /api/collections/:id` - Delete collection
- `POST /api/collections/:id/assets` - Add asset to collection
- `DELETE /api/collections/:id/assets/:asset_id` - Remove asset from collection

### Search

- `GET /api/search?q=query&tags=category:value` - Search assets

### Audit

- `GET /api/audit` - List audit entries
- `GET /api/assets/:id/history` - Get asset change history

### Health

- `GET /health` - Health check

## Tag Categories

Asset classification is entirely tag-based. There is no separate "asset type" field - asset types are handled through the tagging system for maximum flexibility.

- `asset_type` - **Asset type** (document, concept, idea, data_entity, reference, or custom)
- `business_domain` - Marketing, Finance, Legal, etc.
- `language` - English, Farsi, etc.
- `content_format` - markdown, structured data
- `sensitivity_level` - public, internal, confidential
- `source_system` - Origin system
- `structural_type` - structured, semi-structured, unstructured
- `target_user_type` - analysts, developers, executives
- `quality_status` - draft, reviewed, approved
- `custom` - Custom category with name

### Common Asset Type Tag Values

When creating assets, include an `asset_type` tag with one of these common values:
- `document` - Markdown or structured content
- `concept` - Business ideas, technical concepts
- `idea` - Innovation proposals, project concepts
- `data_entity` - Structured data objects
- `reference` - External links, citations
- Custom values as needed by your organization

## Relation Types

- `contains` - Asset includes other assets
- `references` - Asset cites or links to another
- `related_to` - General associations
- `depends_on` - Required relationships
- `supersedes` - Asset replaces another
- `complements` - Assets enhance each other

## License

MIT OR Apache-2.0
