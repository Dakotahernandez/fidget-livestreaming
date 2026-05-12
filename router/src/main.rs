use axum::{
    extract::{ConnectInfo, Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use maxminddb::{geoip2, Reader};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tracing::{info, warn};

/// Path to the GeoLite2 City database file.
const GEOLITE2_DB_PATH: &str = "GeoLite2-City.mmdb";

/// Default port for the router service.
const ROUTER_PORT: u16 = 8070;

/// Represents a geographic coordinate pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Location {
    latitude: f64,
    longitude: f64,
}

/// An edge server registered with the router.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EdgeServer {
    id: String,
    url: String,
    location: Location,
}

/// Response returned by the routing endpoint.
#[derive(Debug, Serialize)]
struct RouteResponse {
    edge_url: String,
    edge_id: String,
    viewer_location: Option<Location>,
}

/// Shared application state containing the GeoIP reader and edge server list.
struct AppState {
    geo_reader: Option<Reader<Vec<u8>>>,
    edges: Vec<EdgeServer>,
}

/// Loads edge server configuration in priority order:
///
/// 1. `EDGE_SERVERS` env var, format `id,url,lat,lon;id,url,lat,lon;...`.
///    Use this for multi-region deployments where the router does
///    geo-aware selection.
/// 2. `CLOUDFRONT_DOMAIN` env var — single edge, location 0/0 (so the
///    router always selects it).
/// 3. In-cluster local edge container at `http://edge:8090` — the
///    sensible default for `docker-compose up` on a developer's box.
fn load_edges_from_env() -> Vec<EdgeServer> {
    if let Ok(raw) = std::env::var("EDGE_SERVERS") {
        let edges: Vec<EdgeServer> = raw
            .split(';')
            .filter(|s| !s.trim().is_empty())
            .filter_map(|entry| {
                let parts: Vec<&str> = entry.split(',').collect();
                if parts.len() != 4 {
                    warn!("Skipping malformed edge entry: {}", entry);
                    return None;
                }
                let lat = parts[2].parse::<f64>().ok()?;
                let lon = parts[3].parse::<f64>().ok()?;
                Some(EdgeServer {
                    id: parts[0].trim().to_string(),
                    url: parts[1].trim().trim_end_matches('/').to_string(),
                    location: Location {
                        latitude: lat,
                        longitude: lon,
                    },
                })
            })
            .collect();

        if !edges.is_empty() {
            return edges;
        }
        warn!("EDGE_SERVERS was set but contained no valid entries; falling back");
    }

    if let Ok(raw) = std::env::var("CLOUDFRONT_DOMAIN") {
        let url = raw.trim().trim_end_matches('/').to_string();
        if !url.is_empty() {
            return vec![EdgeServer {
                id: "cloudfront".to_string(),
                url,
                location: Location {
                    latitude: 0.0,
                    longitude: 0.0,
                },
            }];
        }
    }

    // Local-dev fallback: empty URL means the route handler returns a
    // *relative* `/hls/<key>/index.m3u8` path, which the viewer's browser
    // resolves against its current origin (the frontend container at
    // :3000). The frontend nginx then proxies `/hls/` to the
    // media-server. This avoids handing the browser a Docker-internal
    // hostname like `http://edge:8090/...` that it cannot resolve.
    vec![EdgeServer {
        id: "edge-local".to_string(),
        url: String::new(),
        location: Location {
            latitude: 0.0,
            longitude: 0.0,
        },
    }]
}

/// Looks up the geographic location of an IP address using the GeoLite2 database.
fn lookup_location(reader: &Reader<Vec<u8>>, ip: IpAddr) -> Option<Location> {
    let city: geoip2::City = reader.lookup(ip).ok()?;
    let geo = city.location?;
    Some(Location {
        latitude: geo.latitude?,
        longitude: geo.longitude?,
    })
}

/// Computes the great-circle distance between two points using the Haversine formula.
/// Returns distance in kilometers.
fn haversine_distance(a: &Location, b: &Location) -> f64 {
    const EARTH_RADIUS_KM: f64 = 6371.0;

    let lat1 = a.latitude.to_radians();
    let lat2 = b.latitude.to_radians();
    let dlat = (b.latitude - a.latitude).to_radians();
    let dlon = (b.longitude - a.longitude).to_radians();

    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_KM * h.sqrt().asin()
}

/// Selects the nearest edge server to the viewer's location.
/// Falls back to the first edge in the list if no location is available.
fn select_nearest_edge<'a>(
    edges: &'a [EdgeServer],
    viewer_location: &Option<Location>,
) -> &'a EdgeServer {
    match viewer_location {
        Some(loc) => edges
            .iter()
            .min_by(|a, b| {
                let dist_a = haversine_distance(loc, &a.location);
                let dist_b = haversine_distance(loc, &b.location);
                dist_a
                    .partial_cmp(&dist_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(&edges[0]),
        None => &edges[0],
    }
}

/// Route handler: returns the best edge server URL for a given stream key and viewer IP.
async fn route_viewer(
    Path(stream_key): Path<String>,
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Result<Json<RouteResponse>, StatusCode> {
    if state.edges.is_empty() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }

    let viewer_ip = addr.ip();
    let viewer_location = state
        .geo_reader
        .as_ref()
        .and_then(|reader| lookup_location(reader, viewer_ip));

    let edge = select_nearest_edge(&state.edges, &viewer_location);

    info!(
        stream_key = %stream_key,
        viewer_ip = %viewer_ip,
        selected_edge = %edge.id,
        "Routed viewer to edge"
    );

    Ok(Json(RouteResponse {
        edge_url: format!("{}/{}/index.m3u8", edge.url, stream_key),
        edge_id: edge.id.clone(),
        viewer_location,
    }))
}

async fn health_check() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let geo_reader = match Reader::open_readfile(GEOLITE2_DB_PATH) {
        Ok(reader) => {
            info!("GeoIP database loaded from {}", GEOLITE2_DB_PATH);
            Some(reader)
        }
        Err(e) => {
            warn!(
                "GeoIP database not found at {} ({}). Location-based routing disabled, falling back to first edge.",
                GEOLITE2_DB_PATH, e
            );
            None
        }
    };

    let edges = load_edges_from_env();
    info!("Loaded {} edge server(s)", edges.len());
    for edge in &edges {
        info!(
            "  {} -> {} ({}, {})",
            edge.id, edge.url, edge.location.latitude, edge.location.longitude
        );
    }

    let state = Arc::new(AppState { geo_reader, edges });

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/api/route/:stream_key", get(route_viewer))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], ROUTER_PORT));
    info!("Router listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind router port");
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .expect("Router server crashed");
}
