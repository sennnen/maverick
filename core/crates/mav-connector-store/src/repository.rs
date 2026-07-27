use crate::{
    ApprovalToken, ConnectorSource, InspectionApproval, InstallRequest, InstalledConnector,
    RemovalMode, SourceKind, StateNamespace, StoredState,
};
use mav_connector_runtime::{Artifact, LimitProfile, RevocationSet, TrustPolicy};
use mav_model::error::{codes, MavError, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::path::Path;

const SCHEMA_VERSION: i64 = 3;
const APPROVAL_DOMAIN: &[u8] = b"mavconn-install-approval-v1\0";
const MAX_SOURCE_DISPLAY_BYTES: usize = 256;
const MAX_STATE_BYTES: usize = 64 * 1024;

pub struct ConnectorRepository {
    connection: Connection,
}

struct ActiveArtifact {
    connector_id: String,
    digest: Vec<u8>,
    bytes: Vec<u8>,
}

impl ConnectorRepository {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path).map_err(|source| storage("open", source))?;
        Self::from_connection(connection)
    }

    pub fn open_in_memory() -> Result<Self> {
        let connection = Connection::open_in_memory().map_err(|source| storage("open", source))?;
        Self::from_connection(connection)
    }

    fn from_connection(connection: Connection) -> Result<Self> {
        connection
            .execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")
            .map_err(|source| storage("configure", source))?;
        let mut repository = Self { connection };
        repository.migrate()?;
        Ok(repository)
    }

    fn migrate(&mut self) -> Result<()> {
        self.connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS connector_store_meta (
                    key TEXT PRIMARY KEY NOT NULL,
                    value INTEGER NOT NULL
                 );",
            )
            .map_err(|source| storage("read schema version", source))?;
        let version = self
            .connection
            .query_row(
                "SELECT value FROM connector_store_meta WHERE key = 'schema_version'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|source| storage("read schema version", source))?
            .unwrap_or(0);
        if version > SCHEMA_VERSION {
            return Err(error(
                codes::CONNECTOR_INSTALL_STORAGE,
                format!(
                    "connector store schema {version} is newer than supported {SCHEMA_VERSION}"
                ),
            ));
        }
        if version == 0 {
            self.connection
                .execute_batch(
                    "BEGIN IMMEDIATE;
                     CREATE TABLE connector_source (
                        artifact_digest BLOB PRIMARY KEY NOT NULL,
                        kind TEXT NOT NULL,
                        display_name TEXT NOT NULL,
                        locator_digest BLOB NOT NULL
                     );
                     CREATE TABLE connector_approval (
                        binding BLOB PRIMARY KEY NOT NULL,
                        artifact_digest BLOB NOT NULL,
                        expires_at_ms INTEGER NOT NULL
                     );
                     CREATE TABLE connector_artifact (
                        artifact_digest BLOB PRIMARY KEY NOT NULL,
                        connector_id TEXT NOT NULL,
                        version TEXT NOT NULL,
                        publisher_key_id TEXT NOT NULL,
                        state_schema INTEGER NOT NULL,
                        manifest_digest BLOB NOT NULL,
                        artifact_bytes BLOB NOT NULL,
                        installed_at_ms INTEGER NOT NULL,
                        policy_revision INTEGER NOT NULL,
                        revocation_revision INTEGER NOT NULL,
                        fixture_count INTEGER NOT NULL,
                        disabled_reason TEXT,
                        UNIQUE(connector_id, version),
                        FOREIGN KEY(artifact_digest) REFERENCES connector_source(artifact_digest)
                     );
                     CREATE TABLE connector_activation (
                        connector_id TEXT PRIMARY KEY NOT NULL,
                        artifact_digest BLOB NOT NULL,
                        previous_digest BLOB,
                        activated_at_ms INTEGER NOT NULL,
                        FOREIGN KEY(artifact_digest) REFERENCES connector_artifact(artifact_digest),
                        FOREIGN KEY(previous_digest) REFERENCES connector_artifact(artifact_digest)
                     );
                     CREATE TABLE connector_state (
                        connector_id TEXT NOT NULL,
                        publisher_key_id TEXT NOT NULL,
                        device_id TEXT NOT NULL,
                        state_schema INTEGER NOT NULL,
                        state_bytes BLOB NOT NULL,
                        state_digest BLOB NOT NULL,
                        updated_at_ms INTEGER NOT NULL,
                        PRIMARY KEY(connector_id, publisher_key_id, device_id, state_schema)
                     );
                     CREATE TABLE connector_state_history (
                        artifact_digest BLOB NOT NULL,
                        connector_id TEXT NOT NULL,
                        publisher_key_id TEXT NOT NULL,
                        device_id TEXT NOT NULL,
                        state_schema INTEGER NOT NULL,
                        state_bytes BLOB NOT NULL,
                        state_digest BLOB NOT NULL,
                        updated_at_ms INTEGER NOT NULL,
                        PRIMARY KEY(artifact_digest, connector_id, publisher_key_id, device_id, state_schema)
                     );
                     CREATE TABLE connector_state_quarantine (
                        connector_id TEXT NOT NULL,
                        publisher_key_id TEXT NOT NULL,
                        device_id TEXT NOT NULL,
                        state_schema INTEGER NOT NULL,
                        state_bytes BLOB NOT NULL,
                        state_digest BLOB NOT NULL,
                        quarantined_at_ms INTEGER NOT NULL,
                        PRIMARY KEY(connector_id, publisher_key_id, device_id, state_schema)
                     );
                     CREATE TABLE connector_audit (
                        sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                        occurred_at_ms INTEGER NOT NULL,
                        connector_id TEXT NOT NULL,
                        action TEXT NOT NULL,
                        artifact_digest BLOB,
                        detail TEXT NOT NULL
                     );
                     INSERT INTO connector_store_meta(key, value) VALUES ('schema_version', 1);
                     COMMIT;",
                )
                .map_err(|source| storage("migrate schema", source))?;
        }
        // v2 — the list surfaces had only the connector id and the imported file's name to show,
        // neither of which is what the publisher called the connector. The manifest's own display
        // name is stored so a wearer sees "Generic HR Monitor" rather than
        // `dev.maverick.generic-hr`.
        if version < 2 {
            self.step(2, |connection| {
                connection
                    .execute_batch("ALTER TABLE connector_artifact ADD COLUMN display_name TEXT;")
            })?;
        }
        // v3 — fill that column in for artifacts installed before it existed. Separate from v2 on
        // purpose: adding a column and populating it are two different pieces of work, and a
        // database that already recorded v2 would otherwise never run the second one.
        if version < 3 {
            self.backfill_display_names()?;
            self.step(3, |_| Ok(()))?;
        }
        Ok(())
    }

    /// Apply one migration and record the version it reached, both inside one transaction, so a
    /// failure leaves the store exactly where it started rather than half-migrated.
    fn step(
        &mut self,
        version: i64,
        work: impl FnOnce(&Connection) -> rusqlite::Result<()>,
    ) -> Result<()> {
        let applied = self
            .connection
            .execute_batch("BEGIN IMMEDIATE")
            .and_then(|()| {
                work(&self.connection)
                    .and_then(|()| {
                        self.connection.execute(
                        "INSERT INTO connector_store_meta(key, value) VALUES ('schema_version', ?1)
                         ON CONFLICT(key) DO UPDATE SET value = ?1",
                        [version],
                    )
                    })
                    .and_then(|_| self.connection.execute_batch("COMMIT"))
            });
        applied.map_err(|source| {
            let _ = self.connection.execute_batch("ROLLBACK");
            storage("migrate schema", source)
        })?;
        Ok(())
    }

    /// Read the publisher's name out of every artifact already installed.
    ///
    /// The bytes are in the row beside the column being filled, so the name is recovered rather
    /// than waiting for a reinstall — otherwise every existing wearer keeps seeing a connector id
    /// where a name belongs. An artifact that no longer inspects keeps its id and does not stop
    /// the migration: the store opening is more important than one label.
    fn backfill_display_names(&mut self) -> Result<()> {
        let installed: Vec<(Vec<u8>, Vec<u8>)> = {
            let mut statement = self
                .connection
                .prepare("SELECT artifact_digest, artifact_bytes FROM connector_artifact")
                .map_err(|source| storage("prepare display-name backfill", source))?;
            let rows = statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(|source| storage("read display-name backfill", source))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|source| storage("collect display-name backfill", source))?
        };
        for (digest, bytes) in installed {
            let Ok(artifact) = Artifact::inspect(bytes) else {
                continue;
            };
            self.connection
                .execute(
                    "UPDATE connector_artifact SET display_name = ?2 WHERE artifact_digest = ?1",
                    params![digest, artifact.report().manifest.display_name],
                )
                .map_err(|source| storage("write display-name backfill", source))?;
        }
        Ok(())
    }

    pub fn inspect_connector(
        &self,
        bytes: Vec<u8>,
        source: ConnectorSource,
        policy: &TrustPolicy,
        revocations: &RevocationSet,
        now_ms: i64,
        approval_ttl_ms: i64,
    ) -> Result<InspectionApproval> {
        validate_source(&source)?;
        if approval_ttl_ms <= 0 {
            return Err(error(
                codes::CONNECTOR_INSTALL_APPROVAL_INVALID,
                "approval lifetime must be positive",
            ));
        }
        let expires_at_ms = now_ms.checked_add(approval_ttl_ms).ok_or_else(|| {
            error(
                codes::CONNECTOR_INSTALL_APPROVAL_INVALID,
                "approval expiry overflowed",
            )
        })?;
        let artifact = Artifact::inspect(bytes)?;
        artifact.verify(policy, revocations, now_ms)?;
        let fixtures = artifact.run_fixtures(LimitProfile::mobile_v1())?;
        let report = artifact.report().clone();
        let binding = approval_binding(
            &report.artifact_digest,
            &source,
            policy.revision,
            revocations.revision,
            expires_at_ms,
        );
        self.connection
            .execute(
                "DELETE FROM connector_approval WHERE expires_at_ms < ?1",
                [now_ms],
            )
            .map_err(|source| storage("expire install approvals", source))?;
        self.connection
            .execute(
                "INSERT OR REPLACE INTO connector_approval
                 (binding, artifact_digest, expires_at_ms) VALUES (?1, ?2, ?3)",
                params![
                    binding.as_slice(),
                    report.artifact_digest.as_slice(),
                    expires_at_ms
                ],
            )
            .map_err(|source| storage("issue install approval", source))?;
        Ok(InspectionApproval {
            report,
            fixture_count: fixtures.len() as u32,
            source,
            approval: ApprovalToken {
                binding,
                expires_at_ms,
            },
        })
    }

    pub fn install_connector(
        &mut self,
        request: InstallRequest,
        policy: &TrustPolicy,
        revocations: &RevocationSet,
        now_ms: i64,
    ) -> Result<InstalledConnector> {
        validate_source(&request.source)?;
        let artifact = Artifact::inspect(request.bytes)?;
        artifact.verify(policy, revocations, now_ms)?;
        let fixture_count = artifact.run_fixtures(LimitProfile::mobile_v1())?.len() as u32;
        let report = artifact.report();
        let expected = approval_binding(
            &report.artifact_digest,
            &request.source,
            policy.revision,
            revocations.revision,
            request.approval.expires_at_ms,
        );
        if now_ms > request.approval.expires_at_ms || expected != request.approval.binding {
            return Err(error(
                codes::CONNECTOR_INSTALL_APPROVAL_INVALID,
                "inspection approval is expired or does not bind this install request",
            ));
        }
        let connector_id = report.manifest.connector_id.as_str();
        if let Some(active_version) = self.active_version(connector_id)? {
            if compare_versions(&report.manifest.version, &active_version)? == Ordering::Less {
                return Err(error(
                    codes::CONNECTOR_INSTALL_DOWNGRADE,
                    format!(
                        "connector {connector_id} version {} is older than the active version",
                        report.manifest.version
                    ),
                ));
            }
        }
        let existing = self
            .connection
            .query_row(
                "SELECT a.artifact_digest, s.kind, s.display_name, s.locator_digest
                 FROM connector_artifact a
                 JOIN connector_source s ON s.artifact_digest = a.artifact_digest
                 WHERE a.connector_id = ?1 AND a.version = ?2",
                params![connector_id, report.manifest.version],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Vec<u8>>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|source| storage("check installed version", source))?;
        if let Some((digest, kind, display_name, locator_digest)) = &existing {
            if digest.as_slice() != report.artifact_digest {
                return Err(error(
                    codes::CONNECTOR_INSTALL_APPROVAL_INVALID,
                    "installed connector version has different artifact bytes or publisher",
                ));
            }
            if kind != request.source.kind.as_str()
                || display_name != &request.source.display_name
                || locator_digest.as_slice() != request.source.locator_digest
            {
                return Err(error(
                    codes::CONNECTOR_INSTALL_APPROVAL_INVALID,
                    "reinstall source differs from the artifact's recorded provenance",
                ));
            }
        }

        let transaction = self
            .connection
            .transaction()
            .map_err(|source| storage("begin install", source))?;
        let consumed = transaction
            .execute(
                "DELETE FROM connector_approval
                 WHERE binding = ?1 AND artifact_digest = ?2 AND expires_at_ms = ?3",
                params![
                    request.approval.binding.as_slice(),
                    report.artifact_digest.as_slice(),
                    request.approval.expires_at_ms
                ],
            )
            .map_err(|source| storage("consume install approval", source))?;
        if consumed != 1 {
            return Err(error(
                codes::CONNECTOR_INSTALL_APPROVAL_INVALID,
                "install approval was not issued by this repository or was already consumed",
            ));
        }
        transaction
            .execute(
                "INSERT OR IGNORE INTO connector_source
                 (artifact_digest, kind, display_name, locator_digest) VALUES (?1, ?2, ?3, ?4)",
                params![
                    report.artifact_digest.as_slice(),
                    request.source.kind.as_str(),
                    request.source.display_name,
                    request.source.locator_digest.as_slice()
                ],
            )
            .map_err(|source| storage("store connector source", source))?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO connector_artifact
                 (artifact_digest, connector_id, version, publisher_key_id, state_schema,
                  manifest_digest, artifact_bytes, installed_at_ms, policy_revision,
                  revocation_revision, fixture_count, display_name)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    report.artifact_digest.as_slice(),
                    connector_id,
                    report.manifest.version,
                    report.manifest.publisher_key_id,
                    i64::from(report.manifest.state_schema),
                    report.manifest_digest.as_slice(),
                    artifact.bytes(),
                    now_ms,
                    i64::try_from(policy.revision).map_err(|_| error(
                        codes::CONNECTOR_INSTALL_APPROVAL_INVALID,
                        "trust policy revision exceeds storage range",
                    ))?,
                    i64::try_from(revocations.revision).map_err(|_| error(
                        codes::CONNECTOR_INSTALL_APPROVAL_INVALID,
                        "revocation revision exceeds storage range",
                    ))?,
                    i64::from(fixture_count),
                    report.manifest.display_name,
                ],
            )
            .map_err(|source| storage("store connector artifact", source))?;
        transaction
            .execute(
                "UPDATE connector_artifact SET disabled_reason = NULL
                 WHERE artifact_digest = ?1",
                [report.artifact_digest.as_slice()],
            )
            .map_err(|source| storage("clear reapproved connector disable", source))?;
        audit(
            &transaction,
            now_ms,
            connector_id,
            "install",
            Some(&report.artifact_digest),
            &format!(
                "version={};source={};policy={};revocations={}",
                report.manifest.version,
                request.source.kind.as_str(),
                policy.revision,
                revocations.revision
            ),
        )?;
        if request.activate {
            validate_activation_state(&transaction, connector_id, &report.artifact_digest)?;
            archive_active_state(&transaction, connector_id)?;
            activate_in_transaction(&transaction, connector_id, &report.artifact_digest, now_ms)?;
        }
        transaction
            .commit()
            .map_err(|source| storage("commit install", source))?;
        self.get_by_digest(&report.artifact_digest)
    }

    pub fn list_connectors(&self) -> Result<Vec<InstalledConnector>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT a.connector_id, a.version, a.publisher_key_id, a.state_schema,
                        a.artifact_digest, s.kind, s.display_name, s.locator_digest,
                        a.installed_at_ms, a.policy_revision, a.revocation_revision,
                        a.fixture_count, (x.artifact_digest IS NOT NULL), a.disabled_reason,
                        a.display_name
                 FROM connector_artifact a
                 JOIN connector_source s ON s.artifact_digest = a.artifact_digest
                 LEFT JOIN connector_activation x ON x.artifact_digest = a.artifact_digest
                 ORDER BY a.connector_id, a.installed_at_ms, a.version",
            )
            .map_err(|source| storage("prepare connector list", source))?;
        let rows = statement
            .query_map([], read_installed_row)
            .map_err(|source| storage("query connector list", source))?;
        rows.map(|row| row.map_err(|source| storage("read connector list", source)))
            .collect()
    }

    pub fn activate_connector(
        &mut self,
        connector_id: &str,
        version: &str,
        policy: &TrustPolicy,
        revocations: &RevocationSet,
        now_ms: i64,
    ) -> Result<()> {
        let digest = self.find_digest(connector_id, version)?;
        self.verify_installed(&digest, policy, revocations, now_ms)?;
        let transaction = self
            .connection
            .transaction()
            .map_err(|source| storage("begin activation", source))?;
        validate_activation_state(&transaction, connector_id, &digest)?;
        archive_active_state(&transaction, connector_id)?;
        activate_in_transaction(&transaction, connector_id, &digest, now_ms)?;
        transaction
            .commit()
            .map_err(|source| storage("commit activation", source))
    }

    pub fn rollback_connector(
        &mut self,
        connector_id: &str,
        policy: &TrustPolicy,
        revocations: &RevocationSet,
        now_ms: i64,
    ) -> Result<()> {
        let pair = self
            .connection
            .query_row(
                "SELECT artifact_digest, previous_digest FROM connector_activation
                 WHERE connector_id = ?1",
                [connector_id],
                |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Option<Vec<u8>>>(1)?)),
            )
            .optional()
            .map_err(|source| storage("read rollback target", source))?;
        let Some((current, Some(previous))) = pair else {
            return Err(error(
                codes::CONNECTOR_INSTALL_NOT_FOUND,
                format!("connector {connector_id} has no rollback target"),
            ));
        };
        self.verify_installed(&previous, policy, revocations, now_ms)?;
        let transaction = self
            .connection
            .transaction()
            .map_err(|source| storage("begin rollback", source))?;
        archive_current_state(&transaction, connector_id, &current)?;
        restore_historical_state(&transaction, connector_id, &previous)?;
        transaction
            .execute(
                "UPDATE connector_activation
                 SET artifact_digest = ?1, previous_digest = ?2, activated_at_ms = ?3
                 WHERE connector_id = ?4",
                params![previous, current, now_ms, connector_id],
            )
            .map_err(|source| storage("switch rollback activation", source))?;
        audit(
            &transaction,
            now_ms,
            connector_id,
            "rollback",
            Some(&previous),
            "restored prior activation and state snapshot",
        )?;
        transaction
            .commit()
            .map_err(|source| storage("commit rollback", source))
    }

    pub fn remove_connector(
        &mut self,
        connector_id: &str,
        version: &str,
        mode: RemovalMode,
        policy: &TrustPolicy,
        revocations: &RevocationSet,
        now_ms: i64,
    ) -> Result<()> {
        let digest = self.find_digest(connector_id, version)?;
        let restore_candidate = self
            .connection
            .query_row(
                "SELECT previous_digest FROM connector_activation
                 WHERE connector_id = ?1 AND artifact_digest = ?2",
                params![connector_id, digest],
                |row| row.get::<_, Option<Vec<u8>>>(0),
            )
            .optional()
            .map_err(|source| storage("read removal restoration target", source))?
            .flatten();
        if let Some(previous) = &restore_candidate {
            self.verify_installed(previous, policy, revocations, now_ms)?;
        }
        let transaction = self
            .connection
            .transaction()
            .map_err(|source| storage("begin removal", source))?;
        let activation = transaction
            .query_row(
                "SELECT artifact_digest, previous_digest FROM connector_activation
                 WHERE connector_id = ?1",
                [connector_id],
                |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Option<Vec<u8>>>(1)?)),
            )
            .optional()
            .map_err(|source| storage("check active removal", source))?;
        let removing_active = activation
            .as_ref()
            .is_some_and(|(current, _)| current == &digest);
        let restore = activation
            .as_ref()
            .and_then(|(current, previous)| {
                (current == &digest).then_some(previous.as_ref()).flatten()
            })
            .cloned();
        if let Some(previous) = &restore {
            archive_current_state(&transaction, connector_id, &digest)?;
            restore_historical_state(&transaction, connector_id, previous)?;
            transaction
                .execute(
                    "UPDATE connector_activation
                     SET artifact_digest = ?1, previous_digest = NULL, activated_at_ms = ?2
                     WHERE connector_id = ?3",
                    params![previous, now_ms, connector_id],
                )
                .map_err(|source| storage("restore activation during removal", source))?;
        } else if removing_active {
            transaction
                .execute(
                    "DELETE FROM connector_activation WHERE connector_id = ?1",
                    [connector_id],
                )
                .map_err(|source| storage("remove activation", source))?;
        } else {
            transaction
                .execute(
                    "UPDATE connector_activation SET previous_digest = NULL
                     WHERE connector_id = ?1 AND previous_digest = ?2",
                    params![connector_id, digest],
                )
                .map_err(|source| storage("remove rollback reference", source))?;
        }
        let cleanup_state = removing_active && restore.is_none();
        if cleanup_state && mode == RemovalMode::QuarantineState {
            transaction
                .execute(
                    "INSERT OR REPLACE INTO connector_state_quarantine
                     SELECT connector_id, publisher_key_id, device_id, state_schema,
                            state_bytes, state_digest, ?2
                     FROM connector_state WHERE connector_id = ?1",
                    params![connector_id, now_ms],
                )
                .map_err(|source| storage("quarantine connector state", source))?;
        }
        if cleanup_state {
            transaction
                .execute(
                    "DELETE FROM connector_state WHERE connector_id = ?1",
                    [connector_id],
                )
                .map_err(|source| storage("remove connector state", source))?;
        }
        transaction
            .execute(
                "DELETE FROM connector_state_history WHERE artifact_digest = ?1",
                [digest.as_slice()],
            )
            .map_err(|source| storage("remove connector state history", source))?;
        audit(
            &transaction,
            now_ms,
            connector_id,
            "remove",
            Some(&digest),
            match mode {
                RemovalMode::DeleteState => "state=deleted",
                RemovalMode::QuarantineState => "state=quarantined",
            },
        )?;
        transaction
            .execute(
                "DELETE FROM connector_artifact WHERE artifact_digest = ?1",
                [digest.as_slice()],
            )
            .map_err(|source| storage("remove connector artifact", source))?;
        transaction
            .execute(
                "DELETE FROM connector_source WHERE artifact_digest = ?1",
                [digest.as_slice()],
            )
            .map_err(|source| storage("remove connector source", source))?;
        transaction
            .commit()
            .map_err(|source| storage("commit removal", source))
    }

    pub fn save_state(&mut self, state: &StoredState) -> Result<()> {
        validate_state(state)?;
        let active_digest = self
            .active_digest(&state.namespace.connector_id)?
            .ok_or_else(|| {
                error(
                    codes::CONNECTOR_INSTALL_STATE_NAMESPACE,
                    "connector state namespace has no active connector",
                )
            })?;
        let (publisher, schema) = self.artifact_namespace(&active_digest)?;
        if publisher != state.namespace.publisher_key_id || schema != state.namespace.state_schema {
            return Err(error(
                codes::CONNECTOR_INSTALL_STATE_NAMESPACE,
                "connector state namespace does not match the active artifact",
            ));
        }
        self.connection
            .execute(
                "INSERT INTO connector_state
                 (connector_id, publisher_key_id, device_id, state_schema,
                  state_bytes, state_digest, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(connector_id, publisher_key_id, device_id, state_schema)
                 DO UPDATE SET state_bytes = excluded.state_bytes,
                               state_digest = excluded.state_digest,
                               updated_at_ms = excluded.updated_at_ms",
                params![
                    state.namespace.connector_id,
                    state.namespace.publisher_key_id,
                    state.namespace.device_id,
                    i64::from(state.namespace.state_schema),
                    state.bytes,
                    state.digest.as_slice(),
                    state.updated_at_ms
                ],
            )
            .map_err(|source| storage("save connector state", source))?;
        Ok(())
    }

    pub fn load_state(&self, namespace: &StateNamespace) -> Result<Option<StoredState>> {
        validate_namespace(namespace)?;
        let row = self
            .connection
            .query_row(
                "SELECT state_bytes, state_digest, updated_at_ms FROM connector_state
                 WHERE connector_id = ?1 AND publisher_key_id = ?2
                   AND device_id = ?3 AND state_schema = ?4",
                params![
                    namespace.connector_id,
                    namespace.publisher_key_id,
                    namespace.device_id,
                    i64::from(namespace.state_schema)
                ],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(|source| storage("load connector state", source))?;
        row.map(|(bytes, digest, updated_at_ms)| {
            Ok(StoredState {
                namespace: namespace.clone(),
                bytes,
                digest: digest_array(&digest)?,
                updated_at_ms,
            })
        })
        .transpose()
    }

    pub fn migrate_and_activate<F>(
        &mut self,
        connector_id: &str,
        target_version: &str,
        policy: &TrustPolicy,
        revocations: &RevocationSet,
        now_ms: i64,
        mut migrate: F,
    ) -> Result<()>
    where
        F: FnMut(&StoredState, u32) -> Result<Vec<u8>>,
    {
        let target_digest = self.find_digest(connector_id, target_version)?;
        self.verify_installed(&target_digest, policy, revocations, now_ms)?;
        let (target_publisher, target_schema) = self.artifact_namespace(&target_digest)?;
        let current_digest = self.active_digest(connector_id)?.ok_or_else(|| {
            error(
                codes::CONNECTOR_INSTALL_NOT_FOUND,
                format!("connector {connector_id} has no active version"),
            )
        })?;
        let current_states = self.states_for_connector(connector_id)?;
        let mut replacements = Vec::with_capacity(current_states.len());
        for old in &current_states {
            let bytes = migrate(old, target_schema).map_err(|source| {
                error(
                    codes::CONNECTOR_INSTALL_MIGRATION,
                    format!("connector state migration failed: {}", source.message),
                )
            })?;
            if bytes.len() > MAX_STATE_BYTES {
                return Err(error(
                    codes::CONNECTOR_INSTALL_MIGRATION,
                    "migrated connector state exceeds 64 KiB",
                ));
            }
            replacements.push(StoredState {
                namespace: StateNamespace {
                    connector_id: connector_id.to_owned(),
                    publisher_key_id: target_publisher.clone(),
                    device_id: old.namespace.device_id.clone(),
                    state_schema: target_schema,
                },
                digest: Sha256::digest(&bytes).into(),
                bytes,
                updated_at_ms: now_ms,
            });
        }
        let transaction = self
            .connection
            .transaction()
            .map_err(|source| storage("begin state migration", source))?;
        archive_current_state(&transaction, connector_id, &current_digest)?;
        transaction
            .execute(
                "DELETE FROM connector_state WHERE connector_id = ?1",
                [connector_id],
            )
            .map_err(|source| storage("replace migrated state", source))?;
        for state in &replacements {
            insert_state(&transaction, state)?;
        }
        activate_in_transaction(&transaction, connector_id, &target_digest, now_ms)?;
        audit(
            &transaction,
            now_ms,
            connector_id,
            "migrate",
            Some(&target_digest),
            &format!(
                "state_schema={target_schema};records={}",
                replacements.len()
            ),
        )?;
        transaction
            .commit()
            .map_err(|source| storage("commit state migration", source))
    }

    pub fn enforce_policy(
        &mut self,
        policy: &TrustPolicy,
        revocations: &RevocationSet,
        now_ms: i64,
    ) -> Result<Vec<String>> {
        let active = self.active_artifacts()?;
        let mut disabled = Vec::new();
        for active in active {
            let verdict = Artifact::inspect(active.bytes)
                .and_then(|artifact| artifact.verify(policy, revocations, now_ms));
            if let Err(reason) = verdict {
                let transaction = self
                    .connection
                    .transaction()
                    .map_err(|source| storage("begin policy disable", source))?;
                transaction
                    .execute(
                        "DELETE FROM connector_activation WHERE connector_id = ?1",
                        [&active.connector_id],
                    )
                    .map_err(|source| storage("disable connector", source))?;
                transaction
                    .execute(
                        "UPDATE connector_artifact SET disabled_reason = ?1
                         WHERE artifact_digest = ?2",
                        params![format!("MAV-{}", reason.code), active.digest],
                    )
                    .map_err(|source| storage("record disabled connector", source))?;
                audit(
                    &transaction,
                    now_ms,
                    &active.connector_id,
                    "disable",
                    Some(&active.digest),
                    &format!("reason=MAV-{}", reason.code),
                )?;
                transaction
                    .commit()
                    .map_err(|source| storage("commit policy disable", source))?;
                disabled.push(active.connector_id);
            }
        }
        Ok(disabled)
    }

    pub fn active_artifact(
        &self,
        connector_id: &str,
        policy: &TrustPolicy,
        revocations: &RevocationSet,
        now_ms: i64,
    ) -> Result<Artifact> {
        let digest = self.active_digest(connector_id)?.ok_or_else(|| {
            error(
                codes::CONNECTOR_INSTALL_NOT_FOUND,
                format!("connector {connector_id} has no active artifact"),
            )
        })?;
        self.verified_artifact(&digest, policy, revocations, now_ms)
    }

    fn active_version(&self, connector_id: &str) -> Result<Option<String>> {
        self.connection
            .query_row(
                "SELECT a.version FROM connector_activation x
                 JOIN connector_artifact a ON a.artifact_digest = x.artifact_digest
                 WHERE x.connector_id = ?1",
                [connector_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|source| storage("read active connector version", source))
    }

    fn active_digest(&self, connector_id: &str) -> Result<Option<Vec<u8>>> {
        self.connection
            .query_row(
                "SELECT artifact_digest FROM connector_activation WHERE connector_id = ?1",
                [connector_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|source| storage("read active connector", source))
    }

    fn find_digest(&self, connector_id: &str, version: &str) -> Result<Vec<u8>> {
        self.connection
            .query_row(
                "SELECT artifact_digest FROM connector_artifact
                 WHERE connector_id = ?1 AND version = ?2",
                params![connector_id, version],
                |row| row.get(0),
            )
            .optional()
            .map_err(|source| storage("find installed connector", source))?
            .ok_or_else(|| {
                error(
                    codes::CONNECTOR_INSTALL_NOT_FOUND,
                    format!("connector {connector_id} version {version} is not installed"),
                )
            })
    }

    fn get_by_digest(&self, digest: &[u8; 32]) -> Result<InstalledConnector> {
        self.connection
            .query_row(
                "SELECT a.connector_id, a.version, a.publisher_key_id, a.state_schema,
                        a.artifact_digest, s.kind, s.display_name, s.locator_digest,
                        a.installed_at_ms, a.policy_revision, a.revocation_revision,
                        a.fixture_count, (x.artifact_digest IS NOT NULL), a.disabled_reason,
                        a.display_name
                 FROM connector_artifact a
                 JOIN connector_source s ON s.artifact_digest = a.artifact_digest
                 LEFT JOIN connector_activation x ON x.artifact_digest = a.artifact_digest
                 WHERE a.artifact_digest = ?1",
                [digest.as_slice()],
                read_installed_row,
            )
            .map_err(|source| storage("read installed connector", source))
    }

    fn artifact_namespace(&self, digest: &[u8]) -> Result<(String, u32)> {
        let (publisher, schema) = self
            .connection
            .query_row(
                "SELECT publisher_key_id, state_schema FROM connector_artifact
                 WHERE artifact_digest = ?1",
                [digest],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .map_err(|source| storage("read artifact namespace", source))?;
        Ok((publisher, u32_value(schema)?))
    }

    fn states_for_connector(&self, connector_id: &str) -> Result<Vec<StoredState>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT publisher_key_id, device_id, state_schema, state_bytes,
                        state_digest, updated_at_ms
                 FROM connector_state WHERE connector_id = ?1 ORDER BY device_id, state_schema",
            )
            .map_err(|source| storage("prepare connector states", source))?;
        let rows = statement
            .query_map([connector_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })
            .map_err(|source| storage("query connector states", source))?;
        let mut states = Vec::new();
        for row in rows {
            let (publisher, device, schema, bytes, digest, updated_at_ms) =
                row.map_err(|source| storage("read connector states", source))?;
            states.push(StoredState {
                namespace: StateNamespace {
                    connector_id: connector_id.to_owned(),
                    publisher_key_id: publisher,
                    device_id: device,
                    state_schema: u32_value(schema)?,
                },
                bytes,
                digest: digest_array(&digest)?,
                updated_at_ms,
            });
        }
        Ok(states)
    }

    fn active_artifacts(&self) -> Result<Vec<ActiveArtifact>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT x.connector_id, x.artifact_digest, a.artifact_bytes
                 FROM connector_activation x
                 JOIN connector_artifact a ON a.artifact_digest = x.artifact_digest",
            )
            .map_err(|source| storage("prepare active connectors", source))?;
        let rows = statement
            .query_map([], |row| {
                Ok(ActiveArtifact {
                    connector_id: row.get(0)?,
                    digest: row.get(1)?,
                    bytes: row.get(2)?,
                })
            })
            .map_err(|source| storage("query active connectors", source))?;
        rows.map(|row| row.map_err(|source| storage("read active connectors", source)))
            .collect()
    }

    fn verify_installed(
        &self,
        digest: &[u8],
        policy: &TrustPolicy,
        revocations: &RevocationSet,
        now_ms: i64,
    ) -> Result<()> {
        self.verified_artifact(digest, policy, revocations, now_ms)
            .map(|_| ())
    }

    fn verified_artifact(
        &self,
        digest: &[u8],
        policy: &TrustPolicy,
        revocations: &RevocationSet,
        now_ms: i64,
    ) -> Result<Artifact> {
        let bytes = self
            .connection
            .query_row(
                "SELECT artifact_bytes FROM connector_artifact WHERE artifact_digest = ?1",
                [digest],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(|source| storage("read activation artifact", source))?
            .ok_or_else(|| {
                error(
                    codes::CONNECTOR_INSTALL_NOT_FOUND,
                    "activation artifact is not installed",
                )
            })?;
        let artifact = Artifact::inspect(bytes)?;
        artifact.verify(policy, revocations, now_ms)?;
        artifact.run_fixtures(LimitProfile::mobile_v1())?;
        Ok(artifact)
    }
}

fn activate_in_transaction(
    transaction: &Transaction<'_>,
    connector_id: &str,
    digest: &[u8],
    now_ms: i64,
) -> Result<()> {
    let disabled = transaction
        .query_row(
            "SELECT disabled_reason FROM connector_artifact WHERE artifact_digest = ?1",
            [digest],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|source| storage("validate activation target", source))?;
    match disabled {
        None => {
            return Err(error(
                codes::CONNECTOR_INSTALL_NOT_FOUND,
                format!("connector {connector_id} activation target is not installed"),
            ));
        }
        Some(Some(reason)) => {
            return Err(error(
                codes::CONNECTOR_INSTALL_APPROVAL_INVALID,
                format!("connector {connector_id} is disabled: {reason}"),
            ));
        }
        Some(None) => {}
    }
    transaction
        .execute(
            "INSERT INTO connector_activation
             (connector_id, artifact_digest, previous_digest, activated_at_ms)
             VALUES (?1, ?2, NULL, ?3)
             ON CONFLICT(connector_id) DO UPDATE SET
                artifact_digest = excluded.artifact_digest,
                previous_digest = CASE
                    WHEN connector_activation.artifact_digest = excluded.artifact_digest
                    THEN connector_activation.previous_digest
                    ELSE connector_activation.artifact_digest
                END,
                activated_at_ms = excluded.activated_at_ms",
            params![connector_id, digest, now_ms],
        )
        .map_err(|source| storage("activate connector", source))?;
    audit(
        transaction,
        now_ms,
        connector_id,
        "activate",
        Some(digest),
        "activation switched atomically",
    )
}

fn validate_activation_state(
    transaction: &Transaction<'_>,
    connector_id: &str,
    digest: &[u8],
) -> Result<()> {
    let target = transaction
        .query_row(
            "SELECT publisher_key_id, state_schema FROM connector_artifact
             WHERE artifact_digest = ?1",
            [digest],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .map_err(|source| storage("read activation namespace", source))?;
    let mismatches = transaction
        .query_row(
            "SELECT COUNT(*) FROM connector_state
             WHERE connector_id = ?1 AND (publisher_key_id != ?2 OR state_schema != ?3)",
            params![connector_id, target.0, target.1],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|source| storage("validate activation state", source))?;
    if mismatches != 0 {
        return Err(error(
            codes::CONNECTOR_INSTALL_MIGRATION,
            "connector activation requires an atomic state migration",
        ));
    }
    Ok(())
}

fn archive_current_state(
    transaction: &Transaction<'_>,
    connector_id: &str,
    artifact_digest: &[u8],
) -> Result<()> {
    transaction
        .execute(
            "INSERT OR REPLACE INTO connector_state_history
             SELECT ?2, connector_id, publisher_key_id, device_id, state_schema,
                    state_bytes, state_digest, updated_at_ms
             FROM connector_state WHERE connector_id = ?1",
            params![connector_id, artifact_digest],
        )
        .map_err(|source| storage("archive connector state", source))?;
    Ok(())
}

fn archive_active_state(transaction: &Transaction<'_>, connector_id: &str) -> Result<()> {
    let digest = transaction
        .query_row(
            "SELECT artifact_digest FROM connector_activation WHERE connector_id = ?1",
            [connector_id],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()
        .map_err(|source| storage("read active state snapshot target", source))?;
    if let Some(digest) = digest {
        archive_current_state(transaction, connector_id, &digest)?;
    }
    Ok(())
}

fn restore_historical_state(
    transaction: &Transaction<'_>,
    connector_id: &str,
    artifact_digest: &[u8],
) -> Result<()> {
    transaction
        .execute(
            "DELETE FROM connector_state WHERE connector_id = ?1",
            [connector_id],
        )
        .map_err(|source| storage("clear current connector state", source))?;
    transaction
        .execute(
            "INSERT INTO connector_state
             SELECT connector_id, publisher_key_id, device_id, state_schema,
                    state_bytes, state_digest, updated_at_ms
             FROM connector_state_history
             WHERE connector_id = ?1 AND artifact_digest = ?2",
            params![connector_id, artifact_digest],
        )
        .map_err(|source| storage("restore connector state", source))?;
    Ok(())
}

fn insert_state(transaction: &Transaction<'_>, state: &StoredState) -> Result<()> {
    validate_state(state)?;
    transaction
        .execute(
            "INSERT INTO connector_state
             (connector_id, publisher_key_id, device_id, state_schema,
              state_bytes, state_digest, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                state.namespace.connector_id,
                state.namespace.publisher_key_id,
                state.namespace.device_id,
                i64::from(state.namespace.state_schema),
                state.bytes,
                state.digest.as_slice(),
                state.updated_at_ms
            ],
        )
        .map_err(|source| storage("insert connector state", source))?;
    Ok(())
}

fn audit(
    transaction: &Transaction<'_>,
    now_ms: i64,
    connector_id: &str,
    action: &str,
    artifact_digest: Option<&[u8]>,
    detail: &str,
) -> Result<()> {
    transaction
        .execute(
            "INSERT INTO connector_audit
             (occurred_at_ms, connector_id, action, artifact_digest, detail)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![now_ms, connector_id, action, artifact_digest, detail],
        )
        .map_err(|source| storage("write connector audit", source))?;
    Ok(())
}

fn read_installed_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<InstalledConnector> {
    let artifact_digest = row.get::<_, Vec<u8>>(4)?;
    let locator_digest = row.get::<_, Vec<u8>>(7)?;
    let state_schema = row.get::<_, i64>(3)?;
    let policy_revision = row.get::<_, i64>(9)?;
    let revocation_revision = row.get::<_, i64>(10)?;
    let fixture_count = row.get::<_, i64>(11)?;
    let connector_id: String = row.get(0)?;
    Ok(InstalledConnector {
        display_name: row
            .get::<_, Option<String>>(14)
            .unwrap_or_default()
            .unwrap_or_else(|| connector_id.clone()),
        connector_id,
        version: row.get(1)?,
        publisher_key_id: row.get(2)?,
        state_schema: sql_u32(state_schema, 3)?,
        artifact_digest: vec_to_array(artifact_digest, 4)?,
        source: ConnectorSource {
            kind: match row.get::<_, String>(5)?.as_str() {
                "bundled" => SourceKind::Bundled,
                "imported" => SourceKind::Imported,
                "remote" => SourceKind::Remote,
                other => {
                    return Err(rusqlite::Error::FromSqlConversionFailure(
                        5,
                        rusqlite::types::Type::Text,
                        format!("unknown connector source kind {other}").into(),
                    ));
                }
            },
            display_name: row.get(6)?,
            locator_digest: vec_to_array(locator_digest, 7)?,
        },
        installed_at_ms: row.get(8)?,
        policy_revision: sql_u64(policy_revision, 9)?,
        revocation_revision: sql_u64(revocation_revision, 10)?,
        fixture_count: sql_u32(fixture_count, 11)?,
        active: row.get(12)?,
        disabled_reason: row.get(13)?,
    })
}

fn sql_u32(value: i64, column: usize) -> rusqlite::Result<u32> {
    value
        .try_into()
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
}

fn sql_u64(value: i64, column: usize) -> rusqlite::Result<u64> {
    value
        .try_into()
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
}

fn vec_to_array(bytes: Vec<u8>, column: usize) -> rusqlite::Result<[u8; 32]> {
    bytes.try_into().map_err(|bytes: Vec<u8>| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Blob,
            format!("expected 32-byte digest, got {} bytes", bytes.len()).into(),
        )
    })
}

fn digest_array(bytes: &[u8]) -> Result<[u8; 32]> {
    bytes.try_into().map_err(|_| {
        error(
            codes::CONNECTOR_INSTALL_STORAGE,
            format!("stored digest has invalid length {}", bytes.len()),
        )
    })
}

fn u32_value(value: i64) -> Result<u32> {
    value.try_into().map_err(|_| {
        error(
            codes::CONNECTOR_INSTALL_STORAGE,
            format!("stored state schema {value} is invalid"),
        )
    })
}

fn validate_source(source: &ConnectorSource) -> Result<()> {
    if source.display_name.is_empty()
        || source.display_name.len() > MAX_SOURCE_DISPLAY_BYTES
        || source.display_name.contains('/')
        || source.display_name.contains('\\')
        || source.display_name.contains("://")
    {
        return Err(error(
            codes::CONNECTOR_INSTALL_APPROVAL_INVALID,
            "connector source display name is invalid",
        ));
    }
    Ok(())
}

fn validate_namespace(namespace: &StateNamespace) -> Result<()> {
    if namespace.connector_id.is_empty()
        || namespace.publisher_key_id.is_empty()
        || namespace.device_id.is_empty()
    {
        return Err(error(
            codes::CONNECTOR_INSTALL_STATE_NAMESPACE,
            "connector state namespace components must be non-empty",
        ));
    }
    Ok(())
}

fn validate_state(state: &StoredState) -> Result<()> {
    validate_namespace(&state.namespace)?;
    if state.bytes.len() > MAX_STATE_BYTES
        || Sha256::digest(&state.bytes).as_slice() != state.digest
    {
        return Err(error(
            codes::CONNECTOR_INSTALL_STATE_NAMESPACE,
            "connector state is oversized or its digest does not match",
        ));
    }
    Ok(())
}

fn approval_binding(
    artifact_digest: &[u8; 32],
    source: &ConnectorSource,
    policy_revision: u64,
    revocation_revision: u64,
    expires_at_ms: i64,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(APPROVAL_DOMAIN);
    hasher.update(artifact_digest);
    hasher.update(source.kind.as_str().as_bytes());
    hasher.update((source.display_name.len() as u64).to_be_bytes());
    hasher.update(source.display_name.as_bytes());
    hasher.update(source.locator_digest);
    hasher.update(policy_revision.to_be_bytes());
    hasher.update(revocation_revision.to_be_bytes());
    hasher.update(expires_at_ms.to_be_bytes());
    hasher.finalize().into()
}

fn compare_versions(left: &str, right: &str) -> Result<Ordering> {
    let left = parse_version(left)?;
    let right = parse_version(right)?;
    Ok(left.cmp(&right))
}

fn parse_version(value: &str) -> Result<(u64, u64, u64, VersionPre<'_>)> {
    let (core, pre) = value.split_once('-').unwrap_or((value, ""));
    let mut fields = core.split('.');
    let major = parse_version_field(fields.next(), value)?;
    let minor = parse_version_field(fields.next(), value)?;
    let patch = parse_version_field(fields.next(), value)?;
    if fields.next().is_some() {
        return Err(version_error(value));
    }
    Ok((major, minor, patch, VersionPre(pre)))
}

fn parse_version_field(field: Option<&str>, whole: &str) -> Result<u64> {
    let field = field.ok_or_else(|| version_error(whole))?;
    if field.is_empty() || (field.len() > 1 && field.starts_with('0')) {
        return Err(version_error(whole));
    }
    field.parse().map_err(|_| version_error(whole))
}

#[derive(PartialEq, Eq)]
struct VersionPre<'a>(&'a str);

impl Ord for VersionPre<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.0.is_empty(), other.0.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                for (left, right) in self.0.split('.').zip(other.0.split('.')) {
                    let ordering = match (left.parse::<u64>(), right.parse::<u64>()) {
                        (Ok(left), Ok(right)) => left.cmp(&right),
                        (Ok(_), Err(_)) => Ordering::Less,
                        (Err(_), Ok(_)) => Ordering::Greater,
                        (Err(_), Err(_)) => left.cmp(right),
                    };
                    if ordering != Ordering::Equal {
                        return ordering;
                    }
                }
                self.0.split('.').count().cmp(&other.0.split('.').count())
            }
        }
    }
}

impl PartialOrd for VersionPre<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn version_error(value: &str) -> MavError {
    error(
        codes::CONNECTOR_INSTALL_APPROVAL_INVALID,
        format!("connector version {value} is not comparable semantic versioning"),
    )
}

fn storage(operation: &str, source: rusqlite::Error) -> MavError {
    error(
        codes::CONNECTOR_INSTALL_STORAGE,
        format!("connector store {operation} failed: {source}"),
    )
}

fn error(code: u16, message: impl Into<String>) -> MavError {
    MavError::new(code, message)
}

#[cfg(test)]
mod unit_tests {
    use super::{compare_versions, ConnectorRepository};
    use rusqlite::params;
    use std::cmp::Ordering;

    #[test]
    fn semantic_versions_compare_without_lexical_errors() {
        assert_eq!(
            compare_versions("1.10.0", "1.9.0").ok(),
            Some(Ordering::Greater)
        );
        assert_eq!(
            compare_versions("2.0.0-alpha", "2.0.0").ok(),
            Some(Ordering::Less)
        );
        assert_eq!(
            compare_versions("2.0.0-2", "2.0.0-beta").ok(),
            Some(Ordering::Less)
        );
    }

    #[test]
    fn interrupted_install_rolls_back_at_every_persistence_boundary() {
        for boundary in 1..=4 {
            let mut repository = ConnectorRepository::open_in_memory().expect("repository");
            {
                let transaction = repository.connection.transaction().expect("transaction");
                transaction
                    .execute(
                        "INSERT INTO connector_source
                         (artifact_digest, kind, display_name, locator_digest)
                         VALUES (?1, 'imported', 'fixture', ?2)",
                        params![[1_u8; 32].as_slice(), [2_u8; 32].as_slice()],
                    )
                    .expect("source");
                if boundary > 1 {
                    transaction
                        .execute(
                            "INSERT INTO connector_artifact
                             (artifact_digest, connector_id, version, publisher_key_id,
                              state_schema, manifest_digest, artifact_bytes, installed_at_ms,
                              policy_revision, revocation_revision, fixture_count)
                             VALUES (?1, 'org.example.crash', '1.0.0', 'key', 1,
                                     ?2, x'00', 1, 1, 1, 1)",
                            params![[1_u8; 32].as_slice(), [3_u8; 32].as_slice()],
                        )
                        .expect("artifact");
                }
                if boundary > 2 {
                    transaction
                        .execute(
                            "INSERT INTO connector_activation
                             (connector_id, artifact_digest, previous_digest, activated_at_ms)
                             VALUES ('org.example.crash', ?1, NULL, 1)",
                            [[1_u8; 32].as_slice()],
                        )
                        .expect("activation");
                }
                if boundary > 3 {
                    transaction
                        .execute(
                            "INSERT INTO connector_audit
                             (occurred_at_ms, connector_id, action, artifact_digest, detail)
                             VALUES (1, 'org.example.crash', 'install', ?1, 'boundary')",
                            [[1_u8; 32].as_slice()],
                        )
                        .expect("audit");
                }
            }
            for table in [
                "connector_source",
                "connector_artifact",
                "connector_activation",
                "connector_audit",
            ] {
                let count: i64 = repository
                    .connection
                    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                        row.get(0)
                    })
                    .expect("count");
                assert_eq!(count, 0, "boundary {boundary} left rows in {table}");
            }
        }
    }
}
