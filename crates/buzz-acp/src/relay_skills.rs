//! Progressive-disclosure metadata for owner-assigned shared instructions.

use std::collections::HashMap;

use nostr::{Event, Filter, Kind, PublicKey, SingleLetterTag};

use crate::relay::RestClient;

const SKILL_KIND: u16 = 30023;
const MAX_SKILLS: usize = 64;
const MAX_DESCRIPTION_BYTES: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Coordinate {
    raw: String,
    publisher: PublicKey,
    slug: String,
}

fn parse_coordinate(raw: &str) -> Option<Coordinate> {
    let mut parts = raw.splitn(3, ':');
    if parts.next()? != "30023" {
        return None;
    }
    let publisher = PublicKey::from_hex(parts.next()?).ok()?;
    let slug = parts.next()?.to_string();
    (!slug.is_empty()).then_some(Coordinate {
        raw: raw.to_string(),
        publisher,
        slug,
    })
}

/// Fetch compact, verified covers for exact owner-assigned coordinates.
///
/// Failure is non-fatal at startup: callers omit the section and log the error.
pub(crate) async fn fetch_assigned_skill_covers(
    rest: &RestClient,
    coordinates: &[String],
) -> Result<Option<String>, String> {
    let coordinates = coordinates
        .iter()
        .take(MAX_SKILLS)
        .filter_map(|value| parse_coordinate(value))
        .collect::<Vec<_>>();
    if coordinates.is_empty() {
        return Ok(None);
    }
    let filters = exact_coordinate_filters(&coordinates);
    let value = rest
        .query(&filters)
        .await
        .map_err(|error| format!("shared instruction query failed: {error}"))?;
    let events = value
        .as_array()
        .ok_or_else(|| "relay skill query returned non-array".to_string())?
        .iter()
        .filter_map(|value| serde_json::from_value::<Event>(value.clone()).ok())
        .collect::<Vec<_>>();
    Ok(render_assigned_skill_covers(&coordinates, events))
}

fn exact_coordinate_filters(coordinates: &[Coordinate]) -> Vec<Filter> {
    let d_tag = SingleLetterTag::lowercase(nostr::Alphabet::D);
    coordinates
        .iter()
        .map(|coordinate| {
            Filter::new()
                .kind(Kind::Custom(SKILL_KIND))
                .author(coordinate.publisher)
                .custom_tags(d_tag, [coordinate.slug.as_str()])
                .limit(1)
        })
        .collect()
}

fn render_assigned_skill_covers(coordinates: &[Coordinate], events: Vec<Event>) -> Option<String> {
    let requested = coordinates
        .iter()
        .map(|coordinate| {
            (
                (coordinate.publisher.to_hex(), coordinate.slug.clone()),
                coordinate.raw.clone(),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut heads: HashMap<String, Event> = HashMap::new();
    for event in events {
        if event.kind != Kind::Custom(SKILL_KIND) || event.verify().is_err() {
            continue;
        }
        let Some(slug) = exact_tag(&event, "d") else {
            continue;
        };
        let Some(coordinate) = requested.get(&(event.pubkey.to_hex(), slug.to_string())) else {
            continue;
        };
        let replace = heads.get(coordinate).is_none_or(|current| {
            event.created_at > current.created_at
                || (event.created_at == current.created_at && event.id > current.id)
        });
        if replace {
            heads.insert(coordinate.clone(), event);
        }
    }

    let mut rows = Vec::new();
    for coordinate in coordinates {
        let Some(event) = heads.remove(&coordinate.raw) else {
            continue;
        };
        let Some(summary) = exact_tag(&event, "summary")
            .map(clean_single_line)
            .filter(|value| !value.is_empty() && value.len() <= MAX_DESCRIPTION_BYTES)
        else {
            continue;
        };
        let title = exact_tag(&event, "title")
            .map(clean_single_line)
            .filter(|value| !value.is_empty() && value.len() <= MAX_DESCRIPTION_BYTES)
            .unwrap_or_else(|| coordinate.slug.clone());
        rows.push(format!(
            "- **{title}** (`{}`)\n  {summary}\n  When relevant, fetch the full signed instructions with `buzz notes get --name {} --author {} --content-only`.",
            coordinate.slug,
            coordinate.slug,
            coordinate.publisher.to_hex(),
        ));
    }
    (!rows.is_empty()).then(|| {
        format!(
            "[Assigned Relay Skills]\nThese exact coordinates were assigned by the owner and are authorized for on-demand loading. Read the cover first; fetch a full note only when its description matches the work.\n\n{}",
            rows.join("\n\n")
        )
    })
}

fn exact_tag<'a>(event: &'a Event, name: &str) -> Option<&'a str> {
    let mut values = event.tags.iter().filter_map(|tag| {
        let values = tag.as_slice();
        (values.first().map(String::as_str) == Some(name))
            .then(|| values.get(1).map(String::as_str))
            .flatten()
    });
    let value = values.next()?;
    values.next().is_none().then_some(value)
}

fn clean_single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Tag, Timestamp};

    #[test]
    fn exact_query_filters_do_not_admit_author_slug_cross_products() {
        let alpha = Keys::generate();
        let beta = Keys::generate();
        let coordinate =
            |keys: &Keys, slug: &str| format!("30023:{}:{slug}", keys.public_key().to_hex());
        let coordinates = [coordinate(&alpha, "alpha"), coordinate(&beta, "beta")]
            .iter()
            .filter_map(|value| parse_coordinate(value))
            .collect::<Vec<_>>();

        let filters = exact_coordinate_filters(&coordinates)
            .into_iter()
            .map(|filter| serde_json::to_value(filter).unwrap())
            .collect::<Vec<_>>();

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
    fn renders_only_verified_exact_assigned_covers_in_assignment_order() {
        let first = Keys::generate();
        let second = Keys::generate();
        let coordinate =
            |keys: &Keys, slug: &str| format!("30023:{}:{slug}", keys.public_key().to_hex());
        let note = |keys: &Keys, slug: &str, title: &str, summary: &str, created_at| {
            EventBuilder::new(Kind::Custom(SKILL_KIND), "full body stays lazy")
                .tags([
                    Tag::parse(["d", slug]).unwrap(),
                    Tag::parse(["title", title]).unwrap(),
                    Tag::parse(["summary", summary]).unwrap(),
                ])
                .custom_created_at(Timestamp::from(created_at))
                .sign_with_keys(keys)
                .unwrap()
        };
        let coordinates = [
            coordinate(&second, "review"),
            coordinate(&first, "design-engineering"),
        ]
        .iter()
        .filter_map(|value| parse_coordinate(value))
        .collect::<Vec<_>>();
        let rendered = render_assigned_skill_covers(
            &coordinates,
            vec![
                note(&first, "design-engineering", "Design", "Polish UI", 10),
                note(&second, "review", "Review", "Review code", 20),
                note(&first, "unassigned", "Nope", "Not assigned", 30),
            ],
        )
        .unwrap();

        assert!(rendered.find("**Review**").unwrap() < rendered.find("**Design**").unwrap());
        assert!(rendered.contains("buzz notes get --name review --author"));
        assert!(!rendered.contains("full body stays lazy"));
        assert!(!rendered.contains("Nope"));
    }

    #[test]
    fn oversized_title_falls_back_to_bounded_slug() {
        let author = Keys::generate();
        let coordinate = format!("30023:{}:safe-slug", author.public_key().to_hex());
        let event = EventBuilder::new(Kind::Custom(SKILL_KIND), "lazy body")
            .tags([
                Tag::parse(["d", "safe-slug"]).unwrap(),
                Tag::parse(["title", &"x".repeat(MAX_DESCRIPTION_BYTES + 1)]).unwrap(),
                Tag::parse(["summary", "Bounded summary"]).unwrap(),
            ])
            .sign_with_keys(&author)
            .unwrap();
        let rendered =
            render_assigned_skill_covers(&[parse_coordinate(&coordinate).unwrap()], vec![event])
                .unwrap();

        assert!(rendered.contains("**safe-slug**"));
        assert!(!rendered.contains(&"x".repeat(MAX_DESCRIPTION_BYTES + 1)));
    }
}
