//! Labelled hotel bookings. Reminder wording and delivery times are not new facts.
use chrono::{DateTime, SecondsFormat};
use plugin_sdk::{
    EmailInput, FieldCandidate, RecordContribution, RecordIdentity, RecordResult, SemanticUpdate,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
plugin_sdk::export_plugin!(extract);

fn hash(value: &Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}
fn label<'a>(lines: &'a [String], names: &[&str]) -> Option<&'a str> {
    let values: BTreeSet<_> = lines
        .iter()
        .filter_map(|s| s.split_once(':'))
        .filter(|(k, _)| names.iter().any(|n| k.eq_ignore_ascii_case(n)))
        .map(|(_, v)| v.trim())
        .collect();
    (values.len() == 1)
        .then(|| *values.first().unwrap())
        .filter(|s| !s.is_empty())
}
fn timestamp(s: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.to_utc().to_rfc3339_opts(SecondsFormat::Secs, true))
}

pub fn extract(input: EmailInput) -> RecordResult {
    let mut result = RecordResult {
        schema_version: "1".into(),
        sources: input
            .invocation
            .as_ref()
            .map(|i| i.sources.clone())
            .unwrap_or_default(),
        records: vec![],
        refresh_requests: vec![],
    };
    let Some(source) = input.message.source_id.as_ref().filter(|id| {
        result
            .sources
            .iter()
            .any(|s| &s.id == *id && s.kind == "email")
    }) else {
        return result;
    };
    let Some(observed) = input
        .date
        .as_deref()
        .and_then(|s| {
            DateTime::parse_from_rfc3339(s)
                .or_else(|_| DateTime::parse_from_rfc2822(s))
                .ok()
        })
        .map(|d| d.to_utc().to_rfc3339_opts(SecondsFormat::Secs, true))
    else {
        return result;
    };
    let text = input
        .text_body
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| html_text(input.html_body.as_deref().unwrap_or("")));
    let lines: Vec<_> = text
        .lines()
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    let (Some(hotel), Some(number), Some(check_in), Some(check_out)) = (
        label(&lines, &["hotel", "hotel name"]),
        label(&lines, &["confirmation", "confirmation number"]),
        label(&lines, &["check-in", "check in"]),
        label(&lines, &["check-out", "check out"]),
    ) else {
        return result;
    };
    if hotel.len() > 200
        || !(3..=100).contains(&number.len())
        || !number
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
    {
        return result;
    }
    let (Some(arrival), Some(departure)) = (timestamp(check_in), timestamp(check_out)) else {
        return result;
    };
    if arrival >= departure {
        return result;
    }
    let number = number.to_ascii_uppercase();
    let identity = RecordIdentity {
        namespace: "hotel-reservation".into(),
        key: hash(&json!([hotel.to_lowercase(), number])),
    };
    // Hash instants, not offset formatting or reminder subject text.
    let update_key = format!(
        "reservation:{}",
        hash(&json!([hotel.to_lowercase(), number, arrival, departure]))
    );
    let occurred = input
        .invocation
        .as_ref()
        .and_then(|i| i.records.iter().find(|r| r["identity"] == json!(identity)))
        .and_then(|r| r["updates"][&update_key]["occurredAt"].as_str())
        .unwrap_or(&observed)
        .to_owned();
    let values = BTreeMap::from([
        ("hotelName".into(), json!(hotel)),
        ("confirmationNumber".into(), json!(number)),
        ("checkInAt".into(), json!(arrival)),
        ("checkOutAt".into(), json!(departure)),
        ("checkInLocal".into(), json!(check_in)),
        ("checkOutLocal".into(), json!(check_out)),
    ]);
    let fields: BTreeMap<String, _> = values
        .into_iter()
        .map(|(name, value)| {
            (
                name,
                FieldCandidate {
                    value,
                    evidence: vec![source.clone()],
                    basis: "extracted".into(),
                    confidence: 1.0,
                    observed_at: occurred.clone(),
                    revision: None,
                },
            )
        })
        .collect();
    result.records.push(RecordContribution {
        identity,
        record_type: "hotel-reservation".into(),
        updates: vec![SemanticUpdate {
            key: update_key,
            occurred_at: occurred,
            summary: format!("Hotel stay {number} · {arrival} to {departure}"),
            notify: true,
            evidence: vec![source.clone()],
            fields: ["confirmationNumber", "checkInAt", "checkOutAt"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            supersedes: None,
        }],
        fields,
        attention: vec![],
        links: vec![],
        presentation: json!({"family":"reservation","version":1,"title":hotel}),
        lifecycle: json!({"state":"active"}),
    });
    result
}

fn html_text(html: &str) -> String {
    let mut out = String::new();
    let mut tag = String::new();
    let mut inside = false;
    let mut hidden = false;
    for c in html.chars() {
        match c {
            '<' => {
                inside = true;
                tag.clear();
            }
            '>' if inside => {
                inside = false;
                let name = tag
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if ["script", "style"].contains(&name.as_str()) {
                    hidden = true
                }
                if ["/script", "/style"].contains(&name.as_str()) {
                    hidden = false
                }
                if !hidden
                    && ["br", "br/", "/p", "/div", "/tr", "/li", "/h1", "/h2"]
                        .contains(&name.as_str())
                {
                    out.push('\n')
                }
                if !hidden && ["/td", "/th"].contains(&name.as_str()) {
                    out.push(' ')
                }
            }
            _ if inside => tag.push(c),
            _ if !hidden => out.push(c),
            _ => {}
        }
    }
    html_escape::decode_html_entities(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> EmailInput {
        serde_json::from_str(include_str!("../fixtures/confirmation.json")).unwrap()
    }
    #[test]
    fn ambiguous_or_incomplete_stays_remain_mail() {
        for (old, new) in [
            (
                "Hotel: Harbour Hotel",
                "Hotel: Harbour Hotel\nHotel name: Another Hotel",
            ),
            ("Confirmation number: H-12345", "Confirmation number:"),
            ("2026-11-01T15:00:00-05:00", "2026-11-01"),
            ("2026-11-01T15:00:00-05:00", "2026-11-01T15:00:00"),
            ("2026-11-04T11:00:00-05:00", "2026-02-30T11:00:00-05:00"),
            ("2026-11-04T11:00:00-05:00", "2026-10-31T11:00:00-05:00"),
        ] {
            let mut i = input();
            i.text_body = i.text_body.map(|s| s.replace(old, new));
            assert!(extract(i).records.is_empty(), "{old} -> {new}");
        }
        let mut i = input();
        i.message.source_id = Some("unauthorized".into());
        assert!(extract(i).records.is_empty());
    }
    #[test]
    fn html_and_equivalent_offsets_share_semantic_keys() {
        let original = extract(input());
        let mut html = input();
        html.html_body = html
            .text_body
            .take()
            .map(|s| format!("<p>{}</p>", s.replace('\n', "</p><p>")));
        assert_eq!(json!(extract(html)), json!(original));
        let mut utc = input();
        utc.text_body = utc.text_body.map(|s| {
            s.replace("15:00:00-05:00", "20:00:00Z")
                .replace("11:00:00-05:00", "16:00:00Z")
        });
        let equivalent = extract(utc);
        assert_eq!(
            json!(equivalent.records[0].updates),
            json!(original.records[0].updates)
        );
        for name in &equivalent.records[0].updates[0].fields {
            assert_eq!(
                equivalent.records[0].fields[name].value,
                original.records[0].fields[name].value
            );
        }
    }
}
