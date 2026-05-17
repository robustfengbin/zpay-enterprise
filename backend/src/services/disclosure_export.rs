//! M1.W2 F1.1 polish (2) — render a disclosure body to CSV or PDF.
//!
//! The disclosure JSON body is the authoritative artifact; CSV / PDF are
//! convenience renderings for the auditor's offline archive.  Both
//! formats serialize the `actions[]` table and reproduce the header
//! metadata (wallet_address / granularity / generated_at / resolved_range)
//! so a printed copy is self-explanatory.

use std::io::{BufWriter, Cursor};

use printpdf::{BuiltinFont, Mm, PdfDocument};
use serde_json::Value;

use crate::error::{AppError, AppResult};

const PDF_MAX_ACTIONS: usize = 200;

/// Serialize the disclosure body to CSV.  Header row is fixed across
/// granularities so downstream tools can ingest it uniformly.
pub fn render_csv(body: &Value) -> AppResult<Vec<u8>> {
    let mut wtr = csv::Writer::from_writer(vec![]);
    wtr.write_record([
        "tx_hash",
        "block_height",
        "position_in_block",
        "value_zec",
        "value_zatoshis",
        "memo",
        "nullifier",
        "is_spent",
        "spent_in_tx",
        "recipient_address_hex",
    ])
    .map_err(|e| AppError::InternalError(format!("csv header: {}", e)))?;

    if let Some(actions) = body.get("actions").and_then(|v| v.as_array()) {
        for a in actions {
            wtr.write_record([
                json_str(a, "tx_hash"),
                json_str(a, "block_height"),
                json_str(a, "position_in_block"),
                json_str(a, "value_zec"),
                json_str(a, "value_zatoshis"),
                json_str(a, "memo"),
                json_str(a, "nullifier"),
                json_str(a, "is_spent"),
                json_str(a, "spent_in_tx"),
                json_str(a, "recipient_address_hex"),
            ])
            .map_err(|e| AppError::InternalError(format!("csv row: {}", e)))?;
        }
    }

    wtr.into_inner()
        .map_err(|e| AppError::InternalError(format!("csv flush: {}", e)))
}

/// Serialize the disclosure body to a single-page PDF.  Layout is
/// deliberately spartan — Helvetica 10pt, fixed left margin, one line
/// per action — so the output reads like a printed ledger rather than
/// trying to compete with a real reporting engine.  Caps at
/// PDF_MAX_ACTIONS rows; the JSON / CSV is the authoritative source if
/// the disclosure has more.
pub fn render_pdf(body: &Value) -> AppResult<Vec<u8>> {
    // A4 portrait, 210mm × 297mm.
    let (doc, page_idx, layer_idx) = PdfDocument::new(
        "ZPay Enterprise — Payment Disclosure",
        Mm(210.0),
        Mm(297.0),
        "Layer 1",
    );
    let font = doc
        .add_builtin_font(BuiltinFont::Helvetica)
        .map_err(|e| AppError::InternalError(format!("pdf font: {}", e)))?;
    let font_bold = doc
        .add_builtin_font(BuiltinFont::HelveticaBold)
        .map_err(|e| AppError::InternalError(format!("pdf font: {}", e)))?;

    let current_layer = doc.get_page(page_idx).get_layer(layer_idx);

    // Header block — top of page, descending Y.  We reuse a tiny helper
    // because printpdf positions text by absolute Mm and the y axis runs
    // from the page bottom, so manual bookkeeping is easier here than
    // pulling in a layout engine.
    let mut y: f32 = 280.0;
    let line = |text: &str, y: f32, size: f32, bold: bool| {
        let f = if bold { &font_bold } else { &font };
        current_layer.use_text(text, size, Mm(15.0_f32), Mm(y), f);
    };

    line("Payment Disclosure (ZIP-307 inspired)", y, 14.0, true);
    y -= 7.0;
    line(
        &format!("Generated: {}", json_str(body, "generated_at")),
        y,
        10.0,
        false,
    );
    y -= 5.0;
    line(
        &format!("Wallet: {}", json_str(body, "wallet_address")),
        y,
        10.0,
        false,
    );
    y -= 5.0;
    line(
        &format!(
            "Granularity: {} | Format: {} | Version: {}",
            json_str(body, "granularity"),
            json_str(body, "format"),
            json_str(body, "zip_version")
        ),
        y,
        10.0,
        false,
    );
    y -= 5.0;
    if let Some(rr) = body.get("resolved_range") {
        if !rr.is_null() {
            line(
                &format!(
                    "Range: height {}..{} ({} → {})",
                    json_str(rr, "from_height"),
                    json_str(rr, "to_height"),
                    json_str(rr, "from_ts"),
                    json_str(rr, "to_ts")
                ),
                y,
                10.0,
                false,
            );
            y -= 5.0;
        }
    }
    line(
        &format!("Action count: {}", json_str(body, "action_count")),
        y,
        10.0,
        true,
    );
    y -= 8.0;

    // Column headers.  PDF text positioning is per-call so we hand-place
    // each column at fixed Mm offsets — cheaper than dragging in a table
    // crate just to render ten columns.
    let cols: [(&str, f32); 5] = [
        ("tx_hash (head)", 15.0),
        ("block", 80.0),
        ("ZEC", 100.0),
        ("memo", 125.0),
        ("spent", 175.0),
    ];
    for (label, x) in cols.iter() {
        current_layer.use_text(*label, 9.0, Mm(*x), Mm(y), &font_bold);
    }
    y -= 4.0;
    line(&"-".repeat(95), y, 8.0, false);
    y -= 4.0;

    if let Some(actions) = body.get("actions").and_then(|v| v.as_array()) {
        for (idx, a) in actions.iter().enumerate() {
            if idx >= PDF_MAX_ACTIONS {
                line(
                    &format!(
                        "(truncated at {} rows — full data in CSV / JSON)",
                        PDF_MAX_ACTIONS
                    ),
                    y,
                    9.0,
                    true,
                );
                break;
            }
            if y < 15.0 {
                line("(further rows omitted to fit one page)", y, 9.0, true);
                break;
            }
            let tx_head = json_str(a, "tx_hash");
            let tx_head_short = if tx_head.len() > 16 {
                format!("{}…", &tx_head[..16])
            } else {
                tx_head
            };
            current_layer.use_text(tx_head_short, 8.0, Mm(15.0_f32), Mm(y), &font);
            current_layer.use_text(json_str(a, "block_height"), 8.0, Mm(80.0_f32), Mm(y), &font);
            current_layer.use_text(json_str(a, "value_zec"), 8.0, Mm(100.0_f32), Mm(y), &font);
            let memo = json_str(a, "memo");
            let memo_short = if memo.len() > 28 {
                format!("{}…", &memo[..28])
            } else {
                memo
            };
            current_layer.use_text(memo_short, 8.0, Mm(125.0_f32), Mm(y), &font);
            current_layer.use_text(json_str(a, "is_spent"), 8.0, Mm(175.0_f32), Mm(y), &font);
            y -= 4.0;
        }
    }

    let mut buf: Vec<u8> = Vec::new();
    {
        let cursor = Cursor::new(&mut buf);
        let mut writer = BufWriter::new(cursor);
        doc.save(&mut writer)
            .map_err(|e| AppError::InternalError(format!("pdf save: {}", e)))?;
    }
    Ok(buf)
}

/// Stringify a JSON value field cheaply: strings unquoted, numbers /
/// bools as their JSON form, null/missing as empty string.  Used by
/// both renderers so empty cells render uniformly.
fn json_str(v: &Value, key: &str) -> String {
    match v.get(key) {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}
