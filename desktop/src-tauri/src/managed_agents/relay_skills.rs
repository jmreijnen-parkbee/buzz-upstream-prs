//! Validation and exact-coordinate resolution for relay-backed skills.
//!
//! Assignments always name an exact NIP-23 addressable coordinate. Keeping
//! validation at the persistence boundary prevents slug-only lookups and
//! malformed publisher identities from becoming executable configuration.

use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use nostr::{Event, EventBuilder, Keys, Kind, PublicKey, Tag, Timestamp};
use serde::{Deserialize, Serialize};

use tauri::State;

use crate::{app_state::AppState, native_relay_client::NativeRelayClient};

pub const RELAY_SKILL_KIND: u16 = 30023;
pub const MAX_RELAY_SKILL_SLUG_BYTES: usize = 80;
pub const MAX_AGENT_SKILL_NAME_BYTES: usize = 64;
pub const MAX_AGENT_SKILL_DESCRIPTION_BYTES: usize = 1024;
pub const MAX_ASSIGNED_RELAY_SKILLS: usize = 64;
pub const MAX_RELAY_SKILL_BODY_BYTES: usize = 32 * 1024;
const RELAY_SKILL_FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_MY_RELAY_SKILL_NOTES: usize = 500;
const MAX_RELAY_SKILL_TITLE_BYTES: usize = 280;
const ASSIGNED_RELAY_SKILLS_ENV: &str = "BUZZ_ACP_ASSIGNED_RELAY_SKILLS";

pub(crate) fn apply_assigned_relay_skills_env(
    command: &mut std::process::Command,
    coordinates: &[String],
) {
    if coordinates.is_empty() {
        command.env_remove(ASSIGNED_RELAY_SKILLS_ENV);
    } else {
        command.env(ASSIGNED_RELAY_SKILLS_ENV, coordinates.join(","));
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateRelaySkillInput {
    name: String,
    title: String,
    summary: String,
    instructions: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateRelaySkillInput {
    coordinate: String,
    expected_event_id: String,
    title: String,
    summary: String,
    instructions: String,
}

/// Publish an Agent Skills-compatible kind-30023 note for the active owner.
///
/// Creation intentionally shares the same compatibility validator as discovery,
/// so a successful response is immediately assignable by the picker.
#[tauri::command]
pub(crate) async fn create_relay_skill(
    input: CreateRelaySkillInput,
    state: State<'_, AppState>,
    relay_client: State<'_, NativeRelayClient>,
) -> Result<RelaySkillCover, String> {
    let name = input.name.trim();
    let title = input.title.trim();
    let summary = input.summary.trim();
    let instructions = input.instructions.trim();

    if title.is_empty() {
        return Err("Add a title for this skill".to_string());
    }
    if title.len() > MAX_RELAY_SKILL_TITLE_BYTES {
        return Err(format!(
            "Skill title must be at most {MAX_RELAY_SKILL_TITLE_BYTES} bytes"
        ));
    }
    let incompatibilities = agent_skill_incompatibilities(name, Some(summary), instructions);
    if let Some(reason) = incompatibilities.first() {
        return Err(reason.message.clone());
    }

    let keys = state.signing_keys()?;
    let owner = keys.public_key().to_hex();
    let relay_url = crate::relay::relay_ws_url_with_override(&state);
    let session = relay_client.session(relay_url.clone(), keys.clone()).await;
    let filter = serde_json::json!({
        "kinds": [RELAY_SKILL_KIND],
        "authors": [&owner],
        "#d": [name],
        "limit": 1,
    });
    if !session
        .fetch_events(filter, RELAY_SKILL_FETCH_TIMEOUT)
        .await?
        .is_empty()
    {
        return Err("A skill with this name already exists".to_string());
    }
    let api_base_url = crate::relay::relay_api_base_url_with_override(&state);
    let published_at = Timestamp::now().as_secs().to_string();
    let builder = EventBuilder::new(Kind::Custom(RELAY_SKILL_KIND), instructions).tags([
        Tag::parse(["d", name]).map_err(|error| format!("invalid skill name: {error}"))?,
        Tag::parse(["title", title]).map_err(|error| format!("invalid skill title: {error}"))?,
        Tag::parse(["summary", summary])
            .map_err(|error| format!("invalid skill summary: {error}"))?,
        Tag::parse(["published_at", published_at.as_str()])
            .map_err(|error| format!("invalid publication date: {error}"))?,
    ]);
    let event = builder
        .sign_with_keys(&keys)
        .map_err(|error| format!("failed to sign relay skill: {error}"))?;
    let event_id = event.id.to_hex();
    crate::relay::submit_signed_event_at_with_keys(&event, &state, &api_base_url, &keys).await?;

    if state.signing_keys()?.public_key().to_hex() != owner
        || crate::relay::relay_ws_url_with_override(&state) != relay_url
    {
        return Err("relay skill scope changed while publishing".to_string());
    }

    relay_skill_covers_from_events(&owner, vec![event])
        .into_iter()
        .next()
        .ok_or_else(|| format!("published skill {event_id} could not be verified"))
}

/// Replace one of the active owner's Agent Skills-compatible notes.
///
/// The publisher is taken from the active signing identity and must match the
/// coordinate. The expected event id prevents an edit dialog from silently
/// overwriting a newer version that appeared after it was opened.
#[tauri::command]
pub(crate) async fn update_relay_skill(
    input: UpdateRelaySkillInput,
    state: State<'_, AppState>,
    relay_client: State<'_, NativeRelayClient>,
) -> Result<RelaySkillCover, String> {
    let parsed = parse_relay_skill_coordinate(&input.coordinate)?;
    let title = input.title.trim();
    let summary = input.summary.trim();
    let instructions = input.instructions.trim();
    if title.is_empty() {
        return Err("Add a title for this skill".to_string());
    }
    if title.len() > MAX_RELAY_SKILL_TITLE_BYTES {
        return Err(format!(
            "Skill title must be at most {MAX_RELAY_SKILL_TITLE_BYTES} bytes"
        ));
    }
    let incompatibilities =
        agent_skill_incompatibilities(&parsed.slug, Some(summary), instructions);
    if let Some(reason) = incompatibilities.first() {
        return Err(reason.message.clone());
    }

    let keys = state.signing_keys()?;
    let owner = keys.public_key().to_hex();
    if parsed.publisher.to_hex() != owner {
        return Err("Only the instruction's author can edit it".to_string());
    }

    let relay_url = crate::relay::relay_ws_url_with_override(&state);
    let session = relay_client.session(relay_url.clone(), keys.clone()).await;
    let filter = serde_json::json!({
        "kinds": [RELAY_SKILL_KIND],
        "authors": [&owner],
        "#d": [&parsed.slug],
        "limit": 10,
    });
    let current = resolved_heads_from_events(
        std::slice::from_ref(&input.coordinate),
        session
            .fetch_events(filter, RELAY_SKILL_FETCH_TIMEOUT)
            .await?,
    )
    .into_iter()
    .next()
    .ok_or_else(|| "This instruction no longer exists".to_string())?;
    if current.event_id != input.expected_event_id {
        return Err(
            "This instruction changed after you opened it. Reopen it and try again.".to_string(),
        );
    }

    let published_at = Timestamp::now()
        .as_secs()
        .max(current.updated_at.saturating_add(1));
    let published_at_tag = published_at.to_string();
    let builder = EventBuilder::new(Kind::Custom(RELAY_SKILL_KIND), instructions)
        .tags([
            Tag::parse(["d", parsed.slug.as_str()])
                .map_err(|error| format!("invalid skill name: {error}"))?,
            Tag::parse(["title", title])
                .map_err(|error| format!("invalid skill title: {error}"))?,
            Tag::parse(["summary", summary])
                .map_err(|error| format!("invalid skill summary: {error}"))?,
            Tag::parse(["published_at", published_at_tag.as_str()])
                .map_err(|error| format!("invalid publication date: {error}"))?,
        ])
        .custom_created_at(Timestamp::from(published_at));
    let event = builder
        .sign_with_keys(&keys)
        .map_err(|error| format!("failed to sign relay skill: {error}"))?;
    let event_id = event.id.to_hex();
    let api_base_url = crate::relay::relay_api_base_url_with_override(&state);
    crate::relay::submit_signed_event_at_with_keys(&event, &state, &api_base_url, &keys).await?;

    if state.signing_keys()?.public_key().to_hex() != owner
        || crate::relay::relay_ws_url_with_override(&state) != relay_url
    {
        return Err("relay skill scope changed while publishing".to_string());
    }

    relay_skill_covers_from_events(&owner, vec![event])
        .into_iter()
        .next()
        .ok_or_else(|| format!("updated skill {event_id} could not be verified"))
}

/// List the active user's current kind-30023 notes as compact skill covers.
///
/// Ownership is derived from the active signing identity, never supplied by the
/// frontend. Incompatible notes remain visible with structured reasons so the
/// picker can explain why they cannot be assigned.
#[tauri::command]
pub(crate) async fn list_my_relay_skills(
    state: State<'_, AppState>,
    relay_client: State<'_, NativeRelayClient>,
) -> Result<Vec<RelaySkillCover>, String> {
    let keys = state.signing_keys()?;
    let owner = keys.public_key().to_hex();
    let relay_url = crate::relay::relay_ws_url_with_override(&state);
    let session = relay_client.session(relay_url.clone(), keys).await;
    let filter = serde_json::json!({
        "kinds": [RELAY_SKILL_KIND],
        "authors": [&owner],
        "limit": MAX_MY_RELAY_SKILL_NOTES,
    });
    let events = session
        .fetch_events(filter, RELAY_SKILL_FETCH_TIMEOUT)
        .await?;
    let projection_owner = owner.clone();
    let covers = tauri::async_runtime::spawn_blocking(move || {
        relay_skill_covers_from_events(&projection_owner, events)
    })
    .await
    .map_err(|error| format!("relay skill verification failed: {error}"))?;

    if state.signing_keys()?.public_key().to_hex() != owner
        || crate::relay::relay_ws_url_with_override(&state) != relay_url
    {
        return Err("relay skill scope changed while fetching".to_string());
    }
    Ok(covers)
}

/// Resolve explicitly assigned skills for the active community.
#[tauri::command]
pub(crate) async fn resolve_relay_skills(
    coordinates: Vec<String>,
    state: State<'_, AppState>,
    relay_client: State<'_, NativeRelayClient>,
) -> Result<Vec<ResolvedRelaySkill>, String> {
    let keys = state.signing_keys()?;
    let owner = keys.public_key().to_hex();
    let relay_url = crate::relay::relay_ws_url_with_override(&state);
    let mut resolved =
        resolve_assigned_relay_skills(&relay_client, relay_url.clone(), keys, &coordinates).await?;
    for skill in &mut resolved {
        skill.editable = skill.publisher == owner;
    }
    if state.signing_keys()?.public_key().to_hex() != owner
        || crate::relay::relay_ws_url_with_override(&state) != relay_url
    {
        return Err("relay skill scope changed while fetching".to_string());
    }
    Ok(resolved)
}

/// Validate and normalize exact `30023:<publisher-hex>:<slug>` coordinates.
///
/// Publisher keys are emitted as lowercase hex, while slugs are preserved
/// byte-for-byte. Duplicate coordinates are removed without changing order.
pub fn validate_assigned_relay_skills(coordinates: Vec<String>) -> Result<Vec<String>, String> {
    if coordinates.len() > MAX_ASSIGNED_RELAY_SKILLS {
        return Err(format!(
            "too many assigned relay skills (maximum {MAX_ASSIGNED_RELAY_SKILLS})"
        ));
    }

    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(coordinates.len());
    for coordinate in coordinates {
        let parsed = parse_relay_skill_coordinate(&coordinate)?;
        let value = format!(
            "{RELAY_SKILL_KIND}:{}:{}",
            parsed.publisher.to_hex(),
            parsed.slug
        );
        if seen.insert(value.clone()) {
            normalized.push(value);
        }
    }
    Ok(normalized)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelaySkillCoordinate {
    pub publisher: PublicKey,
    pub slug: String,
}

pub fn parse_relay_skill_coordinate(value: &str) -> Result<RelaySkillCoordinate, String> {
    if value.trim() != value {
        return Err("relay skill coordinate must not have surrounding whitespace".to_string());
    }
    let mut parts = value.splitn(3, ':');
    let kind = parts.next().unwrap_or_default();
    let publisher = parts.next().unwrap_or_default();
    let slug = parts.next().unwrap_or_default();

    if kind != RELAY_SKILL_KIND.to_string() || publisher.is_empty() || slug.is_empty() {
        return Err(format!(
            "invalid relay skill coordinate {value:?}; expected 30023:<publisher-pubkey>:<slug>"
        ));
    }
    if slug.len() > MAX_RELAY_SKILL_SLUG_BYTES {
        return Err(format!(
            "relay skill slug exceeds {MAX_RELAY_SKILL_SLUG_BYTES} bytes"
        ));
    }
    if !slug.chars().all(|character| {
        character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || matches!(character, '-' | '_' | '.')
    }) {
        return Err(
            "relay skill slug may contain only lowercase a-z, 0-9, hyphen, underscore, and dot"
                .to_string(),
        );
    }
    if publisher.len() != 64
        || !publisher
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(
            "relay skill publisher must be a lowercase 64-character hex public key".to_string(),
        );
    }
    let publisher = PublicKey::parse(publisher)
        .map_err(|_| "relay skill publisher must be a valid public key".to_string())?;

    Ok(RelaySkillCoordinate {
        publisher,
        slug: slug.to_string(),
    })
}

/// Compact, verified projection of one current note for the skill picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RelaySkillCover {
    pub coordinate: String,
    pub publisher: String,
    pub slug: String,
    pub title: String,
    pub summary: Option<String>,
    pub event_id: String,
    pub updated_at: u64,
    pub compatible: bool,
    pub incompatibilities: Vec<RelaySkillIncompatibility>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RelaySkillIncompatibility {
    pub code: RelaySkillIncompatibilityCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RelaySkillIncompatibilityCode {
    InvalidName,
    MissingDescription,
    DescriptionTooLong,
    EmptyBody,
    BodyTooLarge,
}

fn agent_skill_incompatibilities(
    slug: &str,
    summary: Option<&str>,
    content: &str,
) -> Vec<RelaySkillIncompatibility> {
    let mut reasons = Vec::new();
    let valid_name = !slug.is_empty()
        && slug.len() <= MAX_AGENT_SKILL_NAME_BYTES
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && !slug.contains("--")
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if !valid_name {
        reasons.push(RelaySkillIncompatibility {
            code: RelaySkillIncompatibilityCode::InvalidName,
            message: format!(
                "Skill name must be 1–{MAX_AGENT_SKILL_NAME_BYTES} characters using lowercase letters, numbers, and single hyphens"
            ),
        });
    }
    match summary.map(str::trim).filter(|value| !value.is_empty()) {
        None => reasons.push(RelaySkillIncompatibility {
            code: RelaySkillIncompatibilityCode::MissingDescription,
            message: "Add a summary so agents can tell when to use this skill".to_string(),
        }),
        Some(summary) if summary.len() > MAX_AGENT_SKILL_DESCRIPTION_BYTES => {
            reasons.push(RelaySkillIncompatibility {
                code: RelaySkillIncompatibilityCode::DescriptionTooLong,
                message: format!(
                    "Summary must be at most {MAX_AGENT_SKILL_DESCRIPTION_BYTES} bytes"
                ),
            });
        }
        Some(_) => {}
    }
    if content.trim().is_empty() {
        reasons.push(RelaySkillIncompatibility {
            code: RelaySkillIncompatibilityCode::EmptyBody,
            message: "Add Markdown instructions to this note".to_string(),
        });
    }
    if content.len() > MAX_RELAY_SKILL_BODY_BYTES {
        reasons.push(RelaySkillIncompatibility {
            code: RelaySkillIncompatibilityCode::BodyTooLarge,
            message: format!("Instructions must be at most {MAX_RELAY_SKILL_BODY_BYTES} bytes"),
        });
    }
    reasons
}

fn relay_skill_covers_from_events(owner: &str, events: Vec<Event>) -> Vec<RelaySkillCover> {
    let mut heads: HashMap<String, Event> = HashMap::new();
    for event in events {
        if event.kind != Kind::Custom(RELAY_SKILL_KIND)
            || event.pubkey.to_hex() != owner
            || event.verify().is_err()
        {
            continue;
        }
        let Some(slug) = exact_single_tag_value(&event, "d").map(str::to_string) else {
            continue;
        };
        let replace = heads.get(&slug).is_none_or(|current| {
            event.created_at > current.created_at
                || (event.created_at == current.created_at && event.id > current.id)
        });
        if replace {
            heads.insert(slug, event);
        }
    }

    let mut covers = heads
        .into_iter()
        .map(|(slug, event)| {
            let summary = bounded_single_line_tag_with_limit(
                &event,
                "summary",
                MAX_AGENT_SKILL_DESCRIPTION_BYTES + 1,
            );
            let incompatibilities =
                agent_skill_incompatibilities(&slug, summary.as_deref(), &event.content);
            RelaySkillCover {
                coordinate: format!("{RELAY_SKILL_KIND}:{owner}:{slug}"),
                publisher: owner.to_string(),
                slug,
                title: bounded_single_line_tag(&event, "title").unwrap_or_default(),
                summary,
                event_id: event.id.to_hex(),
                updated_at: event.created_at.as_secs(),
                compatible: incompatibilities.is_empty(),
                incompatibilities,
            }
        })
        .collect::<Vec<_>>();
    covers.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.slug.cmp(&right.slug))
    });
    covers
}

/// A verified current note for one explicitly assigned coordinate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolvedRelaySkill {
    pub coordinate: String,
    pub publisher: String,
    pub slug: String,
    pub title: String,
    pub summary: Option<String>,
    pub content: String,
    pub event_id: String,
    pub updated_at: u64,
    /// True only when the active signer authored this exact addressable note.
    pub editable: bool,
}

/// Fetch current NIP-23 heads for exact assigned coordinates.
///
/// Each filter names one `(publisher, d)` pair. Keeping those constraints in
/// the same filter avoids the author × slug cross-product that can otherwise
/// let unrelated newer notes consume a limited result page.
pub(crate) async fn resolve_assigned_relay_skills(
    relay_client: &NativeRelayClient,
    relay_url: String,
    keys: Keys,
    coordinates: &[String],
) -> Result<Vec<ResolvedRelaySkill>, String> {
    let requested = validate_assigned_relay_skills(coordinates.to_vec())?;
    if requested.is_empty() {
        return Ok(Vec::new());
    }

    let parsed = requested
        .iter()
        .map(|coordinate| parse_relay_skill_coordinate(coordinate))
        .collect::<Result<Vec<_>, _>>()?;
    let filters = exact_coordinate_filters(&parsed);

    let session = relay_client.session(relay_url, keys).await;
    let pages = futures_util::future::try_join_all(filters.into_iter().map(|filter| {
        let session = &session;
        async move {
            session
                .fetch_events(filter, RELAY_SKILL_FETCH_TIMEOUT)
                .await
        }
    }))
    .await?;
    let events = pages.into_iter().flatten().collect();
    let heads = tauri::async_runtime::spawn_blocking(move || {
        resolved_heads_from_events(&requested, events)
    })
    .await
    .map_err(|error| format!("relay skill verification failed: {error}"))?;
    Ok(heads)
}

fn exact_coordinate_filters(coordinates: &[RelaySkillCoordinate]) -> Vec<serde_json::Value> {
    coordinates
        .iter()
        .map(|coordinate| {
            serde_json::json!({
                "kinds": [RELAY_SKILL_KIND],
                "authors": [coordinate.publisher.to_hex()],
                "#d": [&coordinate.slug],
                "limit": 1,
            })
        })
        .collect()
}

fn resolved_heads_from_events(
    coordinates: &[String],
    events: Vec<Event>,
) -> Vec<ResolvedRelaySkill> {
    let requested = coordinates
        .iter()
        .filter_map(|coordinate| {
            parse_relay_skill_coordinate(coordinate)
                .ok()
                .map(|parsed| ((parsed.publisher.to_hex(), parsed.slug), coordinate.clone()))
        })
        .collect::<HashMap<_, _>>();
    let mut heads: HashMap<String, (Event, String)> = HashMap::new();

    for event in events {
        if event.kind != Kind::Custom(RELAY_SKILL_KIND)
            || event.verify().is_err()
            || event.content.len() > MAX_RELAY_SKILL_BODY_BYTES
        {
            continue;
        }
        let Some(slug) = exact_single_tag_value(&event, "d").map(str::to_string) else {
            continue;
        };
        let key = (event.pubkey.to_hex(), slug.clone());
        let Some(coordinate) = requested.get(&key) else {
            continue;
        };
        let replace = heads.get(coordinate).is_none_or(|(current, _)| {
            event.created_at > current.created_at
                || (event.created_at == current.created_at && event.id > current.id)
        });
        if replace {
            heads.insert(coordinate.clone(), (event, slug));
        }
    }

    coordinates
        .iter()
        .filter_map(|coordinate| {
            let (event, slug) = heads.remove(coordinate)?;
            let publisher = event.pubkey.to_hex();
            let title = bounded_single_line_tag(&event, "title").unwrap_or_default();
            let summary = bounded_single_line_tag(&event, "summary");
            Some(ResolvedRelaySkill {
                coordinate: coordinate.clone(),
                publisher,
                slug,
                title,
                summary,
                content: event.content.clone(),
                event_id: event.id.to_hex(),
                updated_at: event.created_at.as_secs(),
                editable: false,
            })
        })
        .collect()
}

fn bounded_single_line_tag(event: &Event, name: &str) -> Option<String> {
    bounded_single_line_tag_with_limit(event, name, 280)
}

fn bounded_single_line_tag_with_limit(event: &Event, name: &str, limit: usize) -> Option<String> {
    let value = exact_single_tag_value(event, name)?;
    let cleaned = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(limit)
        .collect::<String>();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// Security-relevant coordinate tags must be unambiguous. Metadata follows the
/// same rule so the UI never displays one value while another parser executes.
fn exact_single_tag_value<'a>(event: &'a Event, name: &str) -> Option<&'a str> {
    let mut values = event.tags.iter().filter_map(|tag| {
        let parts = tag.as_slice();
        (parts.first().map(String::as_str) == Some(name))
            .then(|| parts.get(1).map(String::as_str))
            .flatten()
    });
    let value = values.next()?;
    values.next().is_none().then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Tag, Timestamp};

    fn note(keys: &Keys, slug: &str, content: &str, created_at: u64) -> Event {
        EventBuilder::new(Kind::Custom(RELAY_SKILL_KIND), content)
            .tags([
                Tag::parse(["d", slug]).unwrap(),
                Tag::parse(["title", &format!("Title {slug}")]).unwrap(),
                Tag::parse(["summary", &format!("Summary {slug}")]).unwrap(),
            ])
            .custom_created_at(Timestamp::from(created_at))
            .sign_with_keys(keys)
            .unwrap()
    }

    #[test]
    fn assigned_relay_skill_env_is_canonical() {
        let coordinate = format!("30023:{}:review", "a".repeat(64));
        let mut command = std::process::Command::new("buzz-acp");
        command.env(ASSIGNED_RELAY_SKILLS_ENV, "user-supplied");
        apply_assigned_relay_skills_env(&mut command, std::slice::from_ref(&coordinate));
        let env = command
            .get_envs()
            .find(|(key, _)| *key == ASSIGNED_RELAY_SKILLS_ENV)
            .and_then(|(_, value)| value)
            .and_then(|value| value.to_str());
        assert_eq!(env, Some(coordinate.as_str()));
    }

    #[test]
    fn accepts_exact_coordinate_and_deduplicates() {
        let key = "a".repeat(64);
        let coordinate = format!("30023:{key}:design-engineering");
        assert_eq!(
            validate_assigned_relay_skills(vec![coordinate.clone(), coordinate.clone()]).unwrap(),
            vec![coordinate]
        );
    }

    #[test]
    fn rejects_noncanonical_coordinates() {
        let key = "a".repeat(64);
        for invalid in [
            "design-engineering".to_string(),
            format!("30175:{key}:design-engineering"),
            format!("30023:{key}:design\nengineering"),
            format!("30023:{key}:Design-Engineering"),
            format!("30023:{key}:design/engineering"),
            format!("30023:{}:design-engineering", "A".repeat(64)),
            "30023:npub1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqp3d0u5:design-engineering".to_string(),
        ] {
            assert!(
                parse_relay_skill_coordinate(&invalid).is_err(),
                "{invalid:?}"
            );
        }
    }

    #[test]
    fn projects_mine_only_latest_heads_with_explicit_compatibility() {
        let owner = Keys::generate();
        let stranger = Keys::generate();
        let old = note(&owner, "design-engineering", "old body", 10);
        let current = note(&owner, "design-engineering", "current body", 20);
        let invalid_name = note(&owner, "Design_skill", "body", 30);
        let missing_summary = EventBuilder::new(Kind::Custom(RELAY_SKILL_KIND), "body")
            .tags([
                Tag::parse(["d", "missing-summary"]).unwrap(),
                Tag::parse(["title", "Missing summary"]).unwrap(),
            ])
            .custom_created_at(Timestamp::from(40))
            .sign_with_keys(&owner)
            .unwrap();
        let foreign = note(&stranger, "foreign", "body", 50);

        let covers = relay_skill_covers_from_events(
            &owner.public_key().to_hex(),
            vec![old, current, invalid_name, missing_summary, foreign],
        );

        assert_eq!(covers.len(), 3);
        assert_eq!(covers[0].slug, "missing-summary");
        assert!(!covers[0].compatible);
        assert_eq!(
            covers[0].incompatibilities[0].code,
            RelaySkillIncompatibilityCode::MissingDescription
        );
        assert_eq!(covers[1].slug, "Design_skill");
        assert!(!covers[1].compatible);
        assert_eq!(
            covers[1].incompatibilities[0].code,
            RelaySkillIncompatibilityCode::InvalidName
        );
        assert_eq!(covers[2].slug, "design-engineering");
        assert!(covers[2].compatible);
        assert!(covers[2].incompatibilities.is_empty());
        assert_eq!(covers[2].updated_at, 20);
    }

    #[test]
    fn compatibility_rejects_agent_skill_name_edge_cases_and_empty_body() {
        for slug in ["", "-skill", "skill-", "two--hyphens", "with_underscore"] {
            let reasons = agent_skill_incompatibilities(slug, Some("Useful summary"), "body");
            assert!(reasons
                .iter()
                .any(|reason| reason.code == RelaySkillIncompatibilityCode::InvalidName));
        }
        let reasons = agent_skill_incompatibilities("valid-name", Some("summary"), "\n");
        assert_eq!(reasons[0].code, RelaySkillIncompatibilityCode::EmptyBody);
    }

    #[test]
    fn exact_query_filters_do_not_admit_author_slug_cross_products() {
        let alpha = Keys::generate();
        let beta = Keys::generate();
        let coordinates = [
            format!("30023:{}:alpha", alpha.public_key().to_hex()),
            format!("30023:{}:beta", beta.public_key().to_hex()),
        ]
        .iter()
        .map(|value| parse_relay_skill_coordinate(value).unwrap())
        .collect::<Vec<_>>();

        let filters = exact_coordinate_filters(&coordinates);

        assert_eq!(filters.len(), 2);
        assert_eq!(
            filters[0]["authors"],
            serde_json::json!([alpha.public_key().to_hex()])
        );
        assert_eq!(filters[0]["#d"], serde_json::json!(["alpha"]));
        assert_eq!(filters[0]["limit"], serde_json::json!(1));
        assert_eq!(
            filters[1]["authors"],
            serde_json::json!([beta.public_key().to_hex()])
        );
        assert_eq!(filters[1]["#d"], serde_json::json!(["beta"]));
    }

    #[test]
    fn resolves_only_exact_requested_signer_and_slug() {
        let alpha = Keys::generate();
        let beta = Keys::generate();
        let coordinate = format!("30023:{}:design-engineering", alpha.public_key().to_hex());
        let wrong_author_same_slug = note(&beta, "design-engineering", "wrong author", 20);
        let right_author_wrong_slug = note(&alpha, "other", "wrong slug", 30);
        let expected = note(&alpha, "design-engineering", "trusted body", 10);

        let resolved = resolved_heads_from_events(
            std::slice::from_ref(&coordinate),
            vec![
                wrong_author_same_slug,
                right_author_wrong_slug,
                expected.clone(),
            ],
        );

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].coordinate, coordinate);
        assert_eq!(resolved[0].publisher, alpha.public_key().to_hex());
        assert_eq!(resolved[0].slug, "design-engineering");
        assert_eq!(resolved[0].content, "trusted body");
        assert_eq!(
            resolved[0].summary.as_deref(),
            Some("Summary design-engineering")
        );
        assert_eq!(resolved[0].event_id, expected.id.to_hex());
    }

    #[test]
    fn selects_newest_verified_head_and_preserves_assignment_order() {
        let alpha = Keys::generate();
        let beta = Keys::generate();
        let alpha_coordinate = format!("30023:{}:alpha", alpha.public_key().to_hex());
        let beta_coordinate = format!("30023:{}:beta", beta.public_key().to_hex());
        let old_alpha = note(&alpha, "alpha", "old", 10);
        let new_alpha = note(&alpha, "alpha", "new", 20);
        let beta_note = note(&beta, "beta", "beta", 15);

        let resolved = resolved_heads_from_events(
            &[beta_coordinate.clone(), alpha_coordinate.clone()],
            vec![old_alpha, new_alpha, beta_note],
        );

        assert_eq!(
            resolved
                .iter()
                .map(|skill| skill.coordinate.as_str())
                .collect::<Vec<_>>(),
            vec![beta_coordinate.as_str(), alpha_coordinate.as_str()]
        );
        assert_eq!(resolved[1].content, "new");
    }

    #[test]
    fn rejects_tampered_oversize_and_ambiguous_coordinate_events() {
        let keys = Keys::generate();
        let coordinate = format!("30023:{}:skill", keys.public_key().to_hex());
        let mut tampered = note(&keys, "skill", "original", 10);
        tampered.content = "tampered".to_string();
        let oversize = note(
            &keys,
            "skill",
            &"x".repeat(MAX_RELAY_SKILL_BODY_BYTES + 1),
            20,
        );
        let ambiguous = EventBuilder::new(Kind::Custom(RELAY_SKILL_KIND), "ambiguous")
            .tags([
                Tag::parse(["d", "skill"]).unwrap(),
                Tag::parse(["d", "skill"]).unwrap(),
            ])
            .custom_created_at(Timestamp::from(30))
            .sign_with_keys(&keys)
            .unwrap();

        assert!(
            resolved_heads_from_events(&[coordinate], vec![tampered, oversize, ambiguous])
                .is_empty()
        );
    }
}
