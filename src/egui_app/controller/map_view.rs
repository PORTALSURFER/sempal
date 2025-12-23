use super::*;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use rusqlite::types::Value;
use std::collections::HashMap;

pub(crate) struct UmapBounds {
    pub min_x: f32,
    pub max_x: f32,
    pub min_y: f32,
    pub max_y: f32,
}

pub(crate) struct UmapPoint {
    pub sample_id: String,
    pub x: f32,
    pub y: f32,
    pub cluster_id: Option<i32>,
}

impl EguiController {
    pub fn open_map(&mut self) {
        self.ui.map.open = true;
    }

    pub fn build_umap_layout(&mut self, model_id: &str, umap_version: &str) {
        if self.runtime.jobs.umap_build_in_progress() {
            self.set_status("t-SNE build already running", StatusTone::Info);
            return;
        }
        self.runtime.jobs.begin_umap_build(super::jobs::UmapBuildJob {
            model_id: model_id.to_string(),
            umap_version: umap_version.to_string(),
        });
        self.set_status("Building t-SNE layout…", StatusTone::Info);
    }

    pub fn build_umap_clusters(&mut self, model_id: &str, umap_version: &str) {
        if self.runtime.jobs.umap_cluster_build_in_progress() {
            self.set_status("Cluster build already running", StatusTone::Info);
            return;
        }
        let source_id = self.current_source().map(|source| source.id);
        self.runtime
            .jobs
            .begin_umap_cluster_build(super::jobs::UmapClusterBuildJob {
                model_id: model_id.to_string(),
                umap_version: umap_version.to_string(),
                source_id,
            });
        self.set_status("Building clusters…", StatusTone::Info);
    }

    pub fn umap_bounds(
        &mut self,
        model_id: &str,
        umap_version: &str,
        source_id: Option<&SourceId>,
    ) -> Result<Option<UmapBounds>, String> {
        let conn = open_library_db()?;
        load_umap_bounds(&conn, model_id, umap_version, source_id)
    }

    pub fn umap_points_in_bounds(
        &mut self,
        model_id: &str,
        umap_version: &str,
        cluster_method: &str,
        cluster_umap_version: &str,
        source_id: Option<&SourceId>,
        bounds: crate::egui_app::state::MapQueryBounds,
        limit: usize,
    ) -> Result<Vec<UmapPoint>, String> {
        let conn = open_library_db()?;
        load_umap_points(
            &conn,
            model_id,
            umap_version,
            cluster_method,
            cluster_umap_version,
            source_id,
            bounds,
            limit,
        )
    }

    pub fn umap_point_for_sample(
        &mut self,
        model_id: &str,
        umap_version: &str,
        sample_id: &str,
    ) -> Result<Option<(f32, f32)>, String> {
        let conn = open_library_db()?;
        load_umap_point_for_sample(&conn, model_id, umap_version, sample_id)
    }

    pub fn umap_cluster_centroids(
        &mut self,
        model_id: &str,
        umap_version: &str,
        cluster_method: &str,
        cluster_umap_version: &str,
        source_id: Option<&SourceId>,
    ) -> Result<HashMap<i32, crate::egui_app::state::MapClusterCentroid>, String> {
        let conn = open_library_db()?;
        load_umap_cluster_centroids(
            &conn,
            model_id,
            umap_version,
            cluster_method,
            cluster_umap_version,
            source_id,
        )
    }
}

pub(super) fn run_umap_build(model_id: &str, umap_version: &str) -> Result<(), String> {
    let mut conn = open_library_db()?;
    crate::analysis::umap::build_umap_layout(&mut conn, model_id, umap_version, 0, 0.95)?;
    Ok(())
}

pub(super) fn run_umap_cluster_build(
    model_id: &str,
    umap_version: &str,
    source_id: Option<&SourceId>,
) -> Result<crate::analysis::hdbscan::HdbscanStats, String> {
    let mut conn = open_library_db()?;
    let sample_id_prefix = source_id.map(|source_id| format!("{}::%", source_id.as_str()));
    crate::analysis::hdbscan::build_hdbscan_clusters_for_sample_id_prefix(
        &mut conn,
        model_id,
        crate::analysis::hdbscan::HdbscanMethod::Umap,
        Some(umap_version),
        sample_id_prefix.as_deref(),
        crate::analysis::hdbscan::HdbscanConfig {
            min_cluster_size: super::similarity_prep::DEFAULT_CLUSTER_MIN_SIZE,
            min_samples: None,
            allow_single_cluster: false,
        },
    )
}

fn open_library_db() -> Result<Connection, String> {
    crate::sample_sources::library::open_connection()
        .map_err(|err| format!("Open library DB failed: {err}"))
}

fn load_umap_bounds(
    conn: &Connection,
    model_id: &str,
    umap_version: &str,
    source_id: Option<&SourceId>,
) -> Result<Option<UmapBounds>, String> {
    let row = if let Some(source_id) = source_id {
        let prefix = format!("{}::%", source_id.as_str());
        conn.query_row(
            "SELECT MIN(x), MAX(x), MIN(y), MAX(y)
             FROM layout_umap
             WHERE model_id = ?1 AND umap_version = ?2
               AND sample_id LIKE ?3",
            params![model_id, umap_version, prefix],
            |row| {
                let min_x: Option<f32> = row.get(0)?;
                let max_x: Option<f32> = row.get(1)?;
                let min_y: Option<f32> = row.get(2)?;
                let max_y: Option<f32> = row.get(3)?;
                Ok((min_x, max_x, min_y, max_y))
            },
        )
        .optional()
        .map_err(|err| format!("Query t-SNE bounds failed: {err}"))?
    } else {
        conn.query_row(
            "SELECT MIN(x), MAX(x), MIN(y), MAX(y)
             FROM layout_umap
             WHERE model_id = ?1 AND umap_version = ?2",
            params![model_id, umap_version],
            |row| {
                let min_x: Option<f32> = row.get(0)?;
                let max_x: Option<f32> = row.get(1)?;
                let min_y: Option<f32> = row.get(2)?;
                let max_y: Option<f32> = row.get(3)?;
                Ok((min_x, max_x, min_y, max_y))
            },
        )
        .optional()
        .map_err(|err| format!("Query t-SNE bounds failed: {err}"))?
    };
    let Some((min_x, max_x, min_y, max_y)) = row else {
        return Ok(None);
    };
    match (min_x, max_x, min_y, max_y) {
        (Some(min_x), Some(max_x), Some(min_y), Some(max_y)) => Ok(Some(UmapBounds {
            min_x,
            max_x,
            min_y,
            max_y,
        })),
        _ => Ok(None),
    }
}

fn load_umap_points(
    conn: &Connection,
    model_id: &str,
    umap_version: &str,
    cluster_method: &str,
    cluster_umap_version: &str,
    source_id: Option<&SourceId>,
    bounds: crate::egui_app::state::MapQueryBounds,
    limit: usize,
) -> Result<Vec<UmapPoint>, String> {
    let (sql, params) = if let Some(source_id) = source_id {
        let prefix = format!("{}::%", source_id.as_str());
        (
            "SELECT layout_umap.sample_id, layout_umap.x, layout_umap.y, hdbscan_clusters.cluster_id
             FROM layout_umap
             LEFT JOIN hdbscan_clusters
                ON layout_umap.sample_id = hdbscan_clusters.sample_id
               AND hdbscan_clusters.model_id = ?1
               AND hdbscan_clusters.method = ?3
               AND hdbscan_clusters.umap_version = ?4
             WHERE layout_umap.model_id = ?1 AND layout_umap.umap_version = ?2
               AND layout_umap.sample_id LIKE ?5
               AND layout_umap.x >= ?6 AND layout_umap.x <= ?7
               AND layout_umap.y >= ?8 AND layout_umap.y <= ?9
             ORDER BY layout_umap.sample_id ASC
             LIMIT ?10",
            vec![
                Value::Text(model_id.to_string()),
                Value::Text(umap_version.to_string()),
                Value::Text(cluster_method.to_string()),
                Value::Text(cluster_umap_version.to_string()),
                Value::Text(prefix),
                Value::Real(bounds.min_x as f64),
                Value::Real(bounds.max_x as f64),
                Value::Real(bounds.min_y as f64),
                Value::Real(bounds.max_y as f64),
                Value::Integer(limit as i64),
            ],
        )
    } else {
        (
            "SELECT layout_umap.sample_id, layout_umap.x, layout_umap.y, hdbscan_clusters.cluster_id
             FROM layout_umap
             LEFT JOIN hdbscan_clusters
                ON layout_umap.sample_id = hdbscan_clusters.sample_id
               AND hdbscan_clusters.model_id = ?1
               AND hdbscan_clusters.method = ?3
               AND hdbscan_clusters.umap_version = ?4
             WHERE layout_umap.model_id = ?1 AND layout_umap.umap_version = ?2
               AND layout_umap.x >= ?5 AND layout_umap.x <= ?6
               AND layout_umap.y >= ?7 AND layout_umap.y <= ?8
             ORDER BY layout_umap.sample_id ASC
             LIMIT ?9",
            vec![
                Value::Text(model_id.to_string()),
                Value::Text(umap_version.to_string()),
                Value::Text(cluster_method.to_string()),
                Value::Text(cluster_umap_version.to_string()),
                Value::Real(bounds.min_x as f64),
                Value::Real(bounds.max_x as f64),
                Value::Real(bounds.min_y as f64),
                Value::Real(bounds.max_y as f64),
                Value::Integer(limit as i64),
            ],
        )
    };
    let mut stmt = conn
        .prepare(sql)
        .map_err(|err| format!("Prepare layout query failed: {err}"))?;
    let rows = stmt
        .query_map(params_from_iter(params), |row| {
            let cluster_id: Option<i64> = row.get(3)?;
            Ok(UmapPoint {
                sample_id: row.get(0)?,
                x: row.get::<_, f32>(1)?,
                y: row.get::<_, f32>(2)?,
                cluster_id: cluster_id.map(|id| id as i32),
            })
        })
        .map_err(|err| format!("Query layout points failed: {err}"))?;
    let mut points = Vec::new();
    for row in rows {
        points.push(row.map_err(|err| format!("Read layout row failed: {err}"))?);
    }
    Ok(points)
}

fn load_umap_point_for_sample(
    conn: &Connection,
    model_id: &str,
    umap_version: &str,
    sample_id: &str,
) -> Result<Option<(f32, f32)>, String> {
    conn.query_row(
        "SELECT x, y
         FROM layout_umap
         WHERE model_id = ?1 AND umap_version = ?2 AND sample_id = ?3",
        params![model_id, umap_version, sample_id],
        |row| {
            let x: f32 = row.get(0)?;
            let y: f32 = row.get(1)?;
            Ok((x, y))
        },
    )
    .optional()
    .map_err(|err| format!("Query t-SNE point failed: {err}"))
}

fn load_umap_cluster_centroids(
    conn: &Connection,
    model_id: &str,
    umap_version: &str,
    cluster_method: &str,
    cluster_umap_version: &str,
    source_id: Option<&SourceId>,
) -> Result<HashMap<i32, crate::egui_app::state::MapClusterCentroid>, String> {
    let (sql, params) = if let Some(source_id) = source_id {
        let prefix = format!("{}::%", source_id.as_str());
        (
            "SELECT hdbscan_clusters.cluster_id, AVG(layout_umap.x), AVG(layout_umap.y), COUNT(*)
             FROM layout_umap
             JOIN hdbscan_clusters
               ON layout_umap.sample_id = hdbscan_clusters.sample_id
              AND hdbscan_clusters.model_id = ?1
              AND hdbscan_clusters.method = ?3
              AND hdbscan_clusters.umap_version = ?4
             WHERE layout_umap.model_id = ?1 AND layout_umap.umap_version = ?2
               AND layout_umap.sample_id LIKE ?5
             GROUP BY hdbscan_clusters.cluster_id",
            vec![
                Value::Text(model_id.to_string()),
                Value::Text(umap_version.to_string()),
                Value::Text(cluster_method.to_string()),
                Value::Text(cluster_umap_version.to_string()),
                Value::Text(prefix),
            ],
        )
    } else {
        (
            "SELECT hdbscan_clusters.cluster_id, AVG(layout_umap.x), AVG(layout_umap.y), COUNT(*)
             FROM layout_umap
             JOIN hdbscan_clusters
               ON layout_umap.sample_id = hdbscan_clusters.sample_id
              AND hdbscan_clusters.model_id = ?1
              AND hdbscan_clusters.method = ?3
              AND hdbscan_clusters.umap_version = ?4
             WHERE layout_umap.model_id = ?1 AND layout_umap.umap_version = ?2
             GROUP BY hdbscan_clusters.cluster_id",
            vec![
                Value::Text(model_id.to_string()),
                Value::Text(umap_version.to_string()),
                Value::Text(cluster_method.to_string()),
                Value::Text(cluster_umap_version.to_string()),
            ],
        )
    };

    let mut stmt = conn
        .prepare(sql)
        .map_err(|err| format!("Prepare centroid query failed: {err}"))?;
    let rows = stmt
        .query_map(params_from_iter(params), |row| {
            let cluster_id: i64 = row.get(0)?;
            let x: f64 = row.get(1)?;
            let y: f64 = row.get(2)?;
            let count: i64 = row.get(3)?;
            Ok((
                cluster_id as i32,
                crate::egui_app::state::MapClusterCentroid {
                    x: x as f32,
                    y: y as f32,
                    count: count as usize,
                },
            ))
        })
        .map_err(|err| format!("Query centroids failed: {err}"))?;

    let mut centroids = HashMap::new();
    for row in rows {
        let (cluster_id, centroid) =
            row.map_err(|err| format!("Read centroid row failed: {err}"))?;
        centroids.insert(cluster_id, centroid);
    }
    Ok(centroids)
}
