//! Object manifest and metadata types.

use std::error::Error;
use std::fmt;

use kairo_core::canonical::{
    encode_list, encode_option, encode_str, encode_u32, encode_u8, CanonicalEncode,
};
use kairo_core::{BlobId, ObjectId, SnapshotId};
use serde::Deserialize;

/// Canonical ObjectManifest v1 encoding is documented at
/// `schemas/canonical/object-manifest-v1.md`.
const OBJECT_MANIFEST_DOMAIN: &[u8] = b"kairo.object.manifest.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectManifest {
    kairo: KairoManifestSection,
    content: Option<ContentSection>,
    provides: Vec<ProvideDeclaration>,
    dependencies: Vec<DependencyDeclaration>,
}

impl ObjectManifest {
    pub fn parse_toml(input: &str) -> Result<Self, ManifestError> {
        let raw: RawObjectManifest = toml::from_str(input).map_err(ManifestError::Toml)?;
        raw.validate()
    }

    pub fn kairo(&self) -> &KairoManifestSection {
        &self.kairo
    }

    pub fn content(&self) -> Option<&ContentSection> {
        self.content.as_ref()
    }

    pub fn provides(&self) -> &[ProvideDeclaration] {
        &self.provides
    }

    pub fn dependencies(&self) -> &[DependencyDeclaration] {
        &self.dependencies
    }

    pub fn manifest_hash(&self) -> BlobId {
        BlobId::from_bytes(OBJECT_MANIFEST_DOMAIN, &self.canonical_bytes())
    }
}

impl CanonicalEncode for ObjectManifest {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, "ObjectManifest");
        encode_u8(out, 1);
        self.kairo.encode_canonical(out);
        encode_option(out, self.content.as_ref(), |out, content| {
            content.encode_canonical(out);
        });
        encode_list(out, &self.provides, |out, provides| {
            provides.encode_canonical(out);
        });
        encode_list(out, &self.dependencies, |out, dependency| {
            dependency.encode_canonical(out);
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KairoManifestSection {
    schema: u32,
    object: Option<ObjectId>,
    kind: String,
    name: String,
    summary: Option<String>,
}

impl KairoManifestSection {
    pub fn schema(&self) -> u32 {
        self.schema
    }

    pub fn object(&self) -> Option<&ObjectId> {
        self.object.as_ref()
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn summary(&self) -> Option<&str> {
        self.summary.as_deref()
    }
}

impl CanonicalEncode for KairoManifestSection {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_u32(out, self.schema);
        encode_option(out, self.object.as_ref(), |out, object| {
            encode_str(out, object.as_str());
        });
        encode_str(out, &self.kind);
        encode_str(out, &self.name);
        encode_option(out, self.summary.as_ref(), |out, summary| {
            encode_str(out, summary);
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentSection {
    kind: String,
}

impl ContentSection {
    pub fn kind(&self) -> &str {
        &self.kind
    }
}

impl CanonicalEncode for ContentSection {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, &self.kind);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvideDeclaration {
    provides: String,
    version: Option<String>,
}

impl ProvideDeclaration {
    pub fn provides(&self) -> &str {
        &self.provides
    }

    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
}

impl CanonicalEncode for ProvideDeclaration {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, &self.provides);
        encode_option(out, self.version.as_ref(), |out, version| {
            encode_str(out, version);
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyDeclaration {
    Provides(ProvidesDependency),
    Object(ObjectDependency),
}

impl CanonicalEncode for DependencyDeclaration {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            Self::Provides(dependency) => {
                encode_str(out, "provides");
                dependency.encode_canonical(out);
            }
            Self::Object(dependency) => {
                encode_str(out, "object");
                dependency.encode_canonical(out);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvidesDependency {
    provides: String,
}

impl ProvidesDependency {
    pub fn provides(&self) -> &str {
        &self.provides
    }
}

impl CanonicalEncode for ProvidesDependency {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, &self.provides);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectDependency {
    object: ObjectId,
    selector: ObjectDependencySelector,
}

impl ObjectDependency {
    pub fn new(object: ObjectId, selector: ObjectDependencySelector) -> Self {
        Self { object, selector }
    }

    pub fn object(&self) -> &ObjectId {
        &self.object
    }

    pub fn selector(&self) -> &ObjectDependencySelector {
        &self.selector
    }
}

impl CanonicalEncode for ObjectDependency {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, self.object.as_str());
        self.selector.encode_canonical(out);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectDependencySelector {
    Version(String),
    Snapshot(SnapshotId),
}

impl CanonicalEncode for ObjectDependencySelector {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            Self::Version(version) => {
                encode_str(out, "version");
                encode_str(out, version);
            }
            Self::Snapshot(snapshot) => {
                encode_str(out, "snapshot");
                encode_str(out, snapshot.as_str());
            }
        }
    }
}

#[derive(Debug)]
pub enum ManifestError {
    Toml(toml::de::Error),
    InvalidObjectId(kairo_core::IdError),
    InvalidSnapshotId(kairo_core::IdError),
    UnsupportedSchema(u32),
    EmptyField(&'static str),
    UnknownDependencyKind(String),
    MissingObjectDependencyObject,
    MissingObjectDependencySelector,
    ConflictingObjectDependencySelectors,
    MissingProvidesDependency,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Toml(error) => write!(f, "invalid kairo.toml: {error}"),
            Self::InvalidObjectId(error) => write!(f, "invalid object id: {error}"),
            Self::InvalidSnapshotId(error) => write!(f, "invalid snapshot id: {error}"),
            Self::UnsupportedSchema(schema) => write!(f, "unsupported kairo.toml schema {schema}"),
            Self::EmptyField(field) => write!(f, "empty manifest field {field}"),
            Self::UnknownDependencyKind(kind) => write!(f, "unknown dependency kind {kind}"),
            Self::MissingObjectDependencyObject => {
                f.write_str("object dependency is missing object")
            }
            Self::MissingObjectDependencySelector => {
                f.write_str("object dependency must specify version or snapshot")
            }
            Self::ConflictingObjectDependencySelectors => {
                f.write_str("object dependency cannot specify both version and snapshot")
            }
            Self::MissingProvidesDependency => {
                f.write_str("provides dependency is missing provides token")
            }
        }
    }
}

impl Error for ManifestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Toml(error) => Some(error),
            Self::InvalidObjectId(error) | Self::InvalidSnapshotId(error) => Some(error),
            Self::UnsupportedSchema(_)
            | Self::EmptyField(_)
            | Self::UnknownDependencyKind(_)
            | Self::MissingObjectDependencyObject
            | Self::MissingObjectDependencySelector
            | Self::ConflictingObjectDependencySelectors
            | Self::MissingProvidesDependency => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawObjectManifest {
    kairo: RawKairoSection,
    content: Option<RawContentSection>,
    #[serde(default)]
    provides: Vec<RawProvideDeclaration>,
    #[serde(default)]
    dependencies: Vec<RawDependencyDeclaration>,
}

impl RawObjectManifest {
    fn validate(self) -> Result<ObjectManifest, ManifestError> {
        Ok(ObjectManifest {
            kairo: self.kairo.validate()?,
            content: self.content.map(RawContentSection::validate).transpose()?,
            provides: self
                .provides
                .into_iter()
                .map(RawProvideDeclaration::validate)
                .collect::<Result<Vec<_>, _>>()?,
            dependencies: self
                .dependencies
                .into_iter()
                .map(RawDependencyDeclaration::validate)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawKairoSection {
    schema: u32,
    object: Option<String>,
    kind: String,
    name: String,
    summary: Option<String>,
}

impl RawKairoSection {
    fn validate(self) -> Result<KairoManifestSection, ManifestError> {
        if self.schema != 1 {
            return Err(ManifestError::UnsupportedSchema(self.schema));
        }

        ensure_non_empty("kairo.kind", &self.kind)?;
        ensure_non_empty("kairo.name", &self.name)?;

        Ok(KairoManifestSection {
            schema: self.schema,
            object: self
                .object
                .map(ObjectId::new)
                .transpose()
                .map_err(ManifestError::InvalidObjectId)?,
            kind: self.kind,
            name: self.name,
            summary: self.summary,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawContentSection {
    kind: String,
}

impl RawContentSection {
    fn validate(self) -> Result<ContentSection, ManifestError> {
        ensure_non_empty("content.kind", &self.kind)?;

        Ok(ContentSection { kind: self.kind })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvideDeclaration {
    provides: String,
    version: Option<String>,
}

impl RawProvideDeclaration {
    fn validate(self) -> Result<ProvideDeclaration, ManifestError> {
        ensure_non_empty("provides.provides", &self.provides)?;

        Ok(ProvideDeclaration {
            provides: self.provides,
            version: self.version,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDependencyDeclaration {
    kind: String,
    provides: Option<String>,
    object: Option<String>,
    version: Option<String>,
    snapshot: Option<String>,
}

impl RawDependencyDeclaration {
    fn validate(self) -> Result<DependencyDeclaration, ManifestError> {
        match self.kind.as_str() {
            "provides" => self.validate_provides(),
            "object" => self.validate_object(),
            kind => Err(ManifestError::UnknownDependencyKind(kind.to_owned())),
        }
    }

    fn validate_provides(self) -> Result<DependencyDeclaration, ManifestError> {
        let Some(provides) = self.provides else {
            return Err(ManifestError::MissingProvidesDependency);
        };

        ensure_non_empty("dependencies.provides", &provides)?;

        Ok(DependencyDeclaration::Provides(ProvidesDependency {
            provides,
        }))
    }

    fn validate_object(self) -> Result<DependencyDeclaration, ManifestError> {
        let Some(object) = self.object else {
            return Err(ManifestError::MissingObjectDependencyObject);
        };

        let object = ObjectId::new(object).map_err(ManifestError::InvalidObjectId)?;
        let selector = match (self.version, self.snapshot) {
            (Some(version), None) => {
                ensure_non_empty("dependencies.version", &version)?;
                ObjectDependencySelector::Version(version)
            }
            (None, Some(snapshot)) => ObjectDependencySelector::Snapshot(
                SnapshotId::new(snapshot).map_err(ManifestError::InvalidSnapshotId)?,
            ),
            (None, None) => return Err(ManifestError::MissingObjectDependencySelector),
            (Some(_), Some(_)) => return Err(ManifestError::ConflictingObjectDependencySelectors),
        };

        Ok(DependencyDeclaration::Object(ObjectDependency::new(
            object, selector,
        )))
    }
}

fn ensure_non_empty(field: &'static str, value: &str) -> Result<(), ManifestError> {
    if value.is_empty() {
        Err(ManifestError::EmptyField(field))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OBJECT_ID: &str = "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk";
    const SNAPSHOT_ID: &str = "zQmPrz1SBNXD1XAPCaWrTtpBbEZHGevtUoWY8ibihamaApZ";

    #[test]
    fn parses_minimal_manifest() {
        let manifest = ObjectManifest::parse_toml(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [content]
            kind = "tree"
            "#,
        );

        assert!(
            matches!(manifest, Ok(manifest) if manifest.kairo().kind() == "software"
                && manifest.content().map(ContentSection::kind) == Some("tree"))
        );
    }

    #[test]
    fn parses_declared_object_id() {
        let manifest = ObjectManifest::parse_toml(&format!(
            r#"
            [kairo]
            schema = 1
            object = "{OBJECT_ID}"
            kind = "software"
            name = "Example"
            "#
        ));

        assert!(
            matches!(manifest, Ok(manifest) if manifest.kairo().object().map(ObjectId::as_str) == Some(OBJECT_ID))
        );
    }

    #[test]
    fn parses_provides_declaration() {
        let manifest = ObjectManifest::parse_toml(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [[provides]]
            provides = "tool:make"
            version = "3.81"
            "#,
        );

        assert!(
            matches!(manifest, Ok(manifest) if manifest.provides().first().map(ProvideDeclaration::provides) == Some("tool:make"))
        );
    }

    #[test]
    fn parses_provides_dependency() {
        let manifest = ObjectManifest::parse_toml(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [[dependencies]]
            kind = "provides"
            provides = "lib:zlib:static"
            "#,
        );

        assert!(matches!(manifest, Ok(manifest) if matches!(
            manifest.dependencies().first(),
            Some(DependencyDeclaration::Provides(dependency)) if dependency.provides() == "lib:zlib:static"
        )));
    }

    #[test]
    fn parses_object_dependency_with_version() {
        let manifest = ObjectManifest::parse_toml(&format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [[dependencies]]
            kind = "object"
            object = "{OBJECT_ID}"
            version = "^4.1.0"
            "#
        ));

        assert!(matches!(manifest, Ok(manifest) if matches!(
            manifest.dependencies().first(),
            Some(DependencyDeclaration::Object(dependency))
                if dependency.object().as_str() == OBJECT_ID
                    && dependency.selector() == &ObjectDependencySelector::Version("^4.1.0".to_owned())
        )));
    }

    #[test]
    fn parses_object_dependency_with_snapshot() {
        let expected_snapshot = SnapshotId::new(SNAPSHOT_ID);
        let manifest = ObjectManifest::parse_toml(&format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [[dependencies]]
            kind = "object"
            object = "{OBJECT_ID}"
            snapshot = "{SNAPSHOT_ID}"
            "#
        ));

        assert!(
            matches!((manifest, expected_snapshot), (Ok(manifest), Ok(_expected_snapshot)) if matches!(
                manifest.dependencies().first(),
                Some(DependencyDeclaration::Object(dependency))
                    if dependency.object().as_str() == OBJECT_ID
                        && matches!(dependency.selector(), ObjectDependencySelector::Snapshot(snapshot) if snapshot.as_str() == SNAPSHOT_ID)
            ))
        );
    }

    #[test]
    fn rejects_invalid_object_id() {
        let manifest = ObjectManifest::parse_toml(
            r#"
            [kairo]
            schema = 1
            object = "obj_invalid"
            kind = "software"
            name = "Example"
            "#,
        );

        assert!(matches!(manifest, Err(ManifestError::InvalidObjectId(_))));
    }

    #[test]
    fn rejects_object_dependency_without_selector() {
        let manifest = ObjectManifest::parse_toml(&format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [[dependencies]]
            kind = "object"
            object = "{OBJECT_ID}"
            "#
        ));

        assert!(matches!(
            manifest,
            Err(ManifestError::MissingObjectDependencySelector)
        ));
    }

    #[test]
    fn rejects_object_dependency_with_conflicting_selectors() {
        let manifest = ObjectManifest::parse_toml(&format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [[dependencies]]
            kind = "object"
            object = "{OBJECT_ID}"
            version = "^4.1.0"
            snapshot = "{SNAPSHOT_ID}"
            "#
        ));

        assert!(matches!(
            manifest,
            Err(ManifestError::ConflictingObjectDependencySelectors)
        ));
    }

    #[test]
    fn rejects_unsupported_schema() {
        let manifest = ObjectManifest::parse_toml(
            r#"
            [kairo]
            schema = 2
            kind = "software"
            name = "Example"
            "#,
        );

        assert!(matches!(manifest, Err(ManifestError::UnsupportedSchema(2))));
    }

    #[test]
    fn toml_key_order_does_not_affect_manifest_hash() {
        let first = ObjectManifest::parse_toml(&format!(
            r#"
            [kairo]
            schema = 1
            object = "{OBJECT_ID}"
            kind = "software"
            name = "Example"
            summary = "Example object."

            [content]
            kind = "tree"

            [[provides]]
            provides = "tool:make"
            version = "3.81"

            [[dependencies]]
            kind = "object"
            object = "{OBJECT_ID}"
            version = "^4.1.0"
            "#
        ));
        let second = ObjectManifest::parse_toml(&format!(
            r#"
            [[dependencies]]
            version = "^4.1.0"
            object = "{OBJECT_ID}"
            kind = "object"

            [[provides]]
            version = "3.81"
            provides = "tool:make"

            [content]
            kind = "tree"

            [kairo]
            summary = "Example object."
            name = "Example"
            kind = "software"
            object = "{OBJECT_ID}"
            schema = 1
            "#
        ));

        assert!(
            matches!((first, second), (Ok(first), Ok(second)) if first.manifest_hash() == second.manifest_hash())
        );
    }

    #[test]
    fn manifest_hash_changes_when_dependency_changes() {
        let first = ObjectManifest::parse_toml(&format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [[dependencies]]
            kind = "object"
            object = "{OBJECT_ID}"
            version = "^4.1.0"
            "#
        ));
        let second = ObjectManifest::parse_toml(&format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [[dependencies]]
            kind = "object"
            object = "{OBJECT_ID}"
            version = "^4.2.0"
            "#
        ));

        assert!(
            matches!((first, second), (Ok(first), Ok(second)) if first.manifest_hash() != second.manifest_hash())
        );
    }

    #[test]
    fn manifest_hash_is_valid_blob_id() {
        let manifest = ObjectManifest::parse_toml(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"
            "#,
        );

        assert!(
            matches!(manifest.map(|manifest| manifest.manifest_hash()), Ok(hash) if BlobId::new(hash.to_string()) == Ok(hash.clone()))
        );
    }
}
