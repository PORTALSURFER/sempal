use super::*;
use rusqlite::{Connection, OptionalExtension, params};

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

    pub fn umap_bounds(&mut self, model_id: &str, umap_version: &str) -> Result<Option<UmapBounds>, String> {
        let conn = open_library_db()?;
        load_umap_bounds(&conn, model_id, umap_version)
    }

    pub fn umap_points_in_bounds(
        &mut self,
        model_id: &str,
        umap_version: &str,
        cluster_method: &str,
        cluster_umap_version: &str,
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
            bounds,
            limit,
        )
    }
}

fn open_library_db() -> Result<Connection, String> {
    crate::sample_sources::library::open_connection()
        .map_err(|err| format!("Open library DB failed: {err}"))
}

fn load_umap_bounds(
    conn: &Connection,
    model_id: &str,
    umap_version: &str,
) -> Result<Option<UmapBounds>, String> {
    let row = conn
        .query_row(
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
        .map_err(|err| format!("Query UMAP bounds failed: {err}"))?;
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
    bounds: crate::egui_app::state::MapQueryBounds,
    limit: usize,
) -> Result<Vec<UmapPoint>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT layout_umap.sample_id, layout_umap.x, layout_umap.y, hdbscan_clusters.cluster_id
             FROM layout_umap
             LEFT JOIN hdbscan_clusters
                ON layout_umap.sample_id = hdbscan_clusters.sample_id
               AND hdbscan_clusters.model_id = ?1
               AND hdbscan_clusters.method = ?3
               AND hdbscan_clusters.umap_version = ?4
             WHERE layout_umap.model_id = ?1 AND layout_umap.umap_version = ?2
               AND x >= ?5 AND x <= ?6 AND y >= ?7 AND y <= ?8
             ORDER BY sample_id ASC
             LIMIT ?9",
        )
        .map_err(|err| format!("Prepare layout query failed: {err}"))?;
    let rows = stmt
        .query_map(
            params![
                model_id,
                umap_version,
                cluster_method,
                cluster_umap_version,
                bounds.min_x as f64,
                bounds.max_x as f64,
                bounds.min_y as f64,
                bounds.max_y as f64,
                limit as i64,
            ],
            |row| {
                let cluster_id: Option<i64> = row.get(3)?;
                Ok(UmapPoint {
                    sample_id: row.get(0)?,
                    x: row.get::<_, f32>(1)?,
                    y: row.get::<_, f32>(2)?,
                    cluster_id: cluster_id.map(|id| id as i32),
                })
            },
        )
        .map_err(|err| format!("Query layout points failed: {err}"))?;
    let mut points = Vec::new();
    for row in rows {
        points.push(row.map_err(|err| format!("Read layout row failed: {err}"))?);
    }
    Ok(points)
}
