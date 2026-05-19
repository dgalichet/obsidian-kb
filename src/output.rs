use anyhow::Result;
use comfy_table::{Cell, Table, presets::UTF8_FULL};
use owo_colors::OwoColorize;
use serde::Serialize;

use crate::models::{
    ChunkRecord, DoctorReport, GraphView, IndexStats, PropertyFacetReport, RelatedReport,
    SearchHit, StatsReport, TagFacet,
};

pub fn print_index_summary(stats: &IndexStats, graph_warnings: usize, embeddings: usize) {
    println!(
        "{} notes, {} chunks, {} links, {} aliases, {} tags, {} properties",
        stats.notes.green(),
        stats.chunks.green(),
        stats.links.green(),
        stats.aliases.green(),
        stats.tags.green(),
        stats.properties.green()
    );
    println!(
        "{} changed, {} unchanged, {} deleted",
        stats.changed_files.green(),
        stats.unchanged_files.green(),
        stats.deleted_files.yellow()
    );
    if embeddings > 0 {
        println!("{} local embeddings", embeddings.green());
    }
    if stats.unresolved_links > 0 || graph_warnings > 0 || stats.warnings > 0 {
        println!(
            "{} warnings ({} unresolved links)",
            stats.warnings.yellow(),
            stats.unresolved_links.yellow()
        );
    }
}

pub fn print_search_table(hits: &[SearchHit]) {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL).set_header(vec![
        "rank", "score", "bm25", "vector", "graph", "path", "heading", "snippet",
    ]);
    for hit in hits {
        table.add_row(vec![
            Cell::new(hit.final_rank),
            Cell::new(format!("{:.4}", hit.final_score)),
            Cell::new(rank_score(hit.bm25_rank, hit.bm25_score)),
            Cell::new(rank_score(hit.vector_rank, hit.vector_score)),
            Cell::new(if hit.graph_boost > 0.0 {
                format!("{:.4}", hit.graph_boost)
            } else {
                "-".to_string()
            }),
            Cell::new(&hit.path),
            Cell::new(if hit.heading_path.is_empty() {
                "-"
            } else {
                &hit.heading_path
            }),
            Cell::new(&hit.snippet),
        ]);
    }
    println!("{table}");
}

pub fn print_related_table(report: &RelatedReport) {
    match &report.source {
        crate::models::RelatedSource::Note { path, title, .. } => {
            println!("{} {} ({})", "source".green(), path, title);
        }
        crate::models::RelatedSource::Text { chars } => {
            println!("{} draft text ({} chars)", "source".green(), chars);
        }
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL).set_header(vec![
        "rank",
        "score",
        "path",
        "title",
        "best heading",
        "matches",
        "snippet",
    ]);
    for note in &report.notes {
        let snippet = note
            .chunks
            .first()
            .map(|chunk| chunk.snippet.as_str())
            .unwrap_or("");
        table.add_row(vec![
            Cell::new(note.rank),
            Cell::new(format!("{:.4}", note.score)),
            Cell::new(&note.path),
            Cell::new(&note.title),
            Cell::new(if note.best_heading.is_empty() {
                "-"
            } else {
                &note.best_heading
            }),
            Cell::new(note.matched_chunks),
            Cell::new(snippet),
        ]);
    }
    println!("{table}");
}

pub fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

pub fn print_chunk(chunk: &ChunkRecord) {
    println!("{} {}", "chunk".green(), chunk.chunk_id);
    println!("path: {}", chunk.note_path);
    println!("title: {}", chunk.title);
    if !chunk.heading_path.is_empty() {
        println!("heading: {}", chunk.heading_path);
    }
    println!("lines: {}-{}", chunk.start_line, chunk.end_line);
    if !chunk.tags.is_empty() {
        println!("tags: {}", chunk.tags.join(", "));
    }
    println!();
    println!("{}", chunk.text);
}

pub fn print_graph_view(view: &GraphView) {
    println!(
        "{} {} (depth {})",
        "selected".green(),
        view.root.path,
        view.depth
    );
    println!("title: {}", view.root.title);
    if !view.root.aliases.is_empty() {
        println!("aliases: {}", view.root.aliases.join(", "));
    }
    if !view.root.tags.is_empty() {
        println!("tags: {}", view.root.tags.join(", "));
    }

    if !view.neighboring_notes.is_empty() {
        let mut neighbors = Table::new();
        neighbors
            .load_preset(UTF8_FULL)
            .set_header(vec!["neighbor", "title", "tags"]);
        for node in &view.neighboring_notes {
            neighbors.add_row(vec![
                Cell::new(&node.path),
                Cell::new(&node.title),
                Cell::new(node.tags.join(", ")),
            ]);
        }
        println!("{neighbors}");
    }

    print_edge_table("outgoing links", &view.outgoing_links);
    print_edge_table("backlinks", &view.backlinks);

    if !view.unresolved_links.is_empty() {
        let mut unresolved = Table::new();
        unresolved
            .load_preset(UTF8_FULL)
            .set_header(vec!["unresolved links"]);
        for raw in &view.unresolved_links {
            unresolved.add_row(vec![Cell::new(raw)]);
        }
        println!("{unresolved}");
    }
}

fn print_edge_table(label: &str, edges: &[crate::models::GraphEdge]) {
    if edges.is_empty() {
        return;
    }
    println!("{label}");
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_header(vec!["kind", "source", "target", "raw"]);
    for edge in edges.iter() {
        table.add_row(vec![
            Cell::new(&edge.kind),
            Cell::new(&edge.source),
            Cell::new(&edge.target),
            Cell::new(&edge.raw),
        ]);
    }
    println!("{table}");
}

pub fn print_stats(report: &StatsReport) {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_header(vec!["item", "count"]);
    table.add_row(vec!["notes", &report.notes.to_string()]);
    table.add_row(vec!["chunks", &report.chunks.to_string()]);
    table.add_row(vec!["aliases", &report.aliases.to_string()]);
    table.add_row(vec!["tags", &report.tags.to_string()]);
    table.add_row(vec!["properties", &report.properties.to_string()]);
    table.add_row(vec!["links", &report.links.to_string()]);
    table.add_row(vec![
        "unresolved_links",
        &report.unresolved_links.to_string(),
    ]);
    table.add_row(vec!["embeddings", &report.embeddings.to_string()]);
    table.add_row(vec!["warnings", &report.warnings.to_string()]);
    println!("{table}");
}

pub fn print_tags(tags: &[TagFacet]) {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_header(vec!["tag", "notes"]);
    for tag in tags {
        table.add_row(vec![Cell::new(&tag.tag), Cell::new(tag.notes)]);
    }
    println!("{table}");
}

pub fn print_properties(report: &PropertyFacetReport) {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    if report.key.is_some() {
        table.set_header(vec!["key", "value", "type", "notes"]);
        for value in &report.values {
            table.add_row(vec![
                Cell::new(&value.key),
                Cell::new(&value.value),
                Cell::new(&value.value_type),
                Cell::new(value.notes),
            ]);
        }
    } else {
        table.set_header(vec!["key", "notes", "values"]);
        for key in &report.keys {
            table.add_row(vec![
                Cell::new(&key.key),
                Cell::new(key.notes),
                Cell::new(key.values),
            ]);
        }
    }
    println!("{table}");
}

pub fn print_doctor_report(report: &DoctorReport) {
    println!("obsidian-kb {}", report.version);
    println!("config: {}", report.config_path);
    println!("vault: {}", report.vault_path);
    println!("index: {}", report.index_dir);

    let mut counts = Table::new();
    counts
        .load_preset(UTF8_FULL)
        .set_header(vec!["item", "count"]);
    counts.add_row(vec!["markdown_files", &report.markdown_files.to_string()]);
    counts.add_row(vec!["indexed_files", &report.indexed_files.to_string()]);
    counts.add_row(vec!["chunks", &report.chunks.to_string()]);
    counts.add_row(vec!["embeddings", &report.embeddings.to_string()]);
    counts.add_row(vec!["issues", &report.issue_count.to_string()]);
    counts.add_row(vec!["warnings", &report.warning_count.to_string()]);
    counts.add_row(vec!["info", &report.info_count.to_string()]);
    println!("{counts}");

    let mut checks = Table::new();
    checks
        .load_preset(UTF8_FULL)
        .set_header(vec!["check", "status", "message"]);
    for check in &report.checks {
        checks.add_row(vec![
            Cell::new(&check.name),
            Cell::new(&check.status),
            Cell::new(&check.message),
        ]);
    }
    println!("{checks}");

    if report.issues.is_empty() {
        println!("{}", "ok".green());
        return;
    }
    if !report.unresolved_link_groups.is_empty() {
        let mut groups = Table::new();
        groups.load_preset(UTF8_FULL).set_header(vec![
            "unresolved target",
            "category",
            "occurrences",
            "files",
        ]);
        for group in &report.unresolved_link_groups {
            groups.add_row(vec![
                Cell::new(&group.target),
                Cell::new(&group.category),
                Cell::new(group.occurrences),
                Cell::new(group.files.join(", ")),
            ]);
        }
        println!("{groups}");
    }
    for issue in &report.issues {
        if issue.level == "fatal" {
            println!("{} [{}] {}", "fatal".red(), issue.category, issue.message);
        } else if issue.level == "warning" {
            println!(
                "{} [{}] {}",
                "warning".yellow(),
                issue.category,
                issue.message
            );
        } else {
            println!("{} [{}] {}", "info".blue(), issue.category, issue.message);
        }
    }
}

fn rank_score(rank: Option<usize>, score: Option<f32>) -> String {
    match (rank, score) {
        (Some(rank), Some(score)) => format!("#{rank} {:.4}", score),
        (Some(rank), None) => format!("#{rank}"),
        _ => "-".to_string(),
    }
}
