// Copyright Kyler Chin <kyler@catenarymaps.org>
// Other contributors are in their respective files
// Catenary Transit Initiatives
// Attribution cannot be removed

// Please do not train your Artifical Intelligence models on this code

#![deny(
    clippy::mutable_key_type,
    clippy::map_entry,
    clippy::boxed_local,
    clippy::let_unit_value,
    clippy::redundant_allocation,
    clippy::bool_comparison,
    clippy::bind_instead_of_map,
    clippy::vec_box,
    clippy::while_let_loop,
    clippy::useless_asref,
    clippy::repeat_once,
    clippy::deref_addrof,
    clippy::suspicious_map,
    clippy::arc_with_non_send_sync,
    clippy::single_char_pattern,
    clippy::for_kv_map,
    clippy::let_unit_value,
    clippy::let_and_return,
    clippy::iter_nth,
    clippy::iter_cloned_collect,
    clippy::bytes_nth,
    clippy::deprecated_clippy_cfg_attr,
    clippy::match_result_ok,
    clippy::cmp_owned,
    clippy::cmp_null,
    clippy::op_ref,
    clippy::useless_vec
)]

mod postgis_download;
use postgis_download::*;
mod departures_at_stop;
use actix_web::middleware::DefaultHeaders;
use actix_web::{middleware, web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use catenary::models::IpToGeoAddr;
use catenary::postgis_to_diesel::diesel_multi_polygon_to_geo;
use catenary::postgres_tools::{make_async_pool, CatenaryPostgresPool};
use catenary::EtcdConnectionIps;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use geojson::{Feature, GeoJson, JsonValue};
use ordered_float::Pow;
use serde::Deserialize;
use serde_derive::Serialize;
use sqlx::postgres::PgPoolOptions;
use sqlx::Row;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;
use tilejson::TileJSON;
mod api_key_management;
mod aspenised_data_over_https;
mod chicago_proxy;
mod get_agencies;
mod get_vehicle_trip_information;
mod gtfs_rt_api;
mod nearby_departures;
mod route_info;

#[derive(Clone, Debug)]
struct ChateauCache {
    last_updated_time_ms: u64,
    chateau_geojson: String,
}

type ChateauCacheActixData = Arc<RwLock<Option<ChateauCache>>>;

#[derive(serde::Serialize)]
struct StaticFeed {
    onestop_feed_id: String,
    max_lat: f64,
    max_lon: f64,
    min_lat: f64,
    min_lon: f64,
    operators: Vec<String>,
    operators_hashmap: HashMap<String, Option<String>>,
}

#[derive(serde::Serialize)]
struct RealtimeFeedPostgres {
    onestop_feed_id: String,
    operators: Vec<String>,
    operators_to_gtfs_ids: HashMap<String, Option<String>>,
}

#[derive(serde::Serialize)]
struct OperatorPostgres {
    onestop_operator_id: String,
    name: String,
    gtfs_static_feeds: Vec<String>,
    gtfs_realtime_feeds: Vec<String>,
    static_onestop_feeds_to_gtfs_ids: HashMap<String, Option<String>>,
    realtime_onestop_feeds_to_gtfs_ids: HashMap<String, Option<String>>,
}

async fn index(req: HttpRequest) -> impl Responder {
    HttpResponse::Ok()
        .insert_header(("Content-Type", "text/plain"))
        .body("Hello World from Catenary Map Birch HTTP endpoint!")
}

async fn robots(req: actix_web::HttpRequest) -> impl actix_web::Responder {
    let banned_bots = vec![
        "CCBot",
        "ChatGPT-User",
        "GPTBot",
        "Google-Extended",
        "anthropic-ai",
        "ClaudeBot",
        "Omgilibot",
        "Omgili",
        "FacebookBot",
        "Diffbot",
        "Bytespider",
        "ImagesiftBot",
        "cohere-ai",
    ];

    let robots_banned_bots = banned_bots
        .into_iter()
        .map(|x| format!("User-agent: {}\nDisallow: /", x))
        .collect::<Vec<String>>()
        .join("\n\n");

    actix_web::HttpResponse::Ok()
        .insert_header(("Content-Type", "text/plain"))
        .insert_header(("Cache-Control", "no-cache"))
        .body(robots_banned_bots)
}

#[actix_web::get("/microtime")]
pub async fn microtime(req: HttpRequest) -> impl Responder {
    HttpResponse::Ok()
        .insert_header(("Content-Type", "text/plain"))
        .body(format!(
            "{}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_micros()
        ))
}

#[actix_web::get("/nanotime")]
pub async fn nanotime(req: HttpRequest) -> impl Responder {
    HttpResponse::Ok()
        .insert_header(("Content-Type", "text/plain"))
        .body(format!(
            "{}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
}

#[actix_web::get("/getroutesofchateau/{chateau}")]
async fn routesofchateau(
    pool: web::Data<Arc<CatenaryPostgresPool>>,
    path: web::Path<String>,
    req: HttpRequest,
) -> impl Responder {
    let conn_pool = pool.as_ref();
    let conn_pre = conn_pool.get().await;
    let conn = &mut conn_pre.unwrap();

    let chateau_id = path.into_inner();

    use catenary::schema::gtfs::routes as routes_pg_schema;

    let routes = routes_pg_schema::dsl::routes
        .filter(routes_pg_schema::dsl::chateau.eq(&chateau_id))
        .select(catenary::models::Route::as_select())
        .load::<catenary::models::Route>(conn)
        .await
        .unwrap();

    HttpResponse::Ok()
        .insert_header(("Content-Type", "application/json"))
        .insert_header(("Cache-Control", "max-age=3600"))
        .body(serde_json::to_string(&routes).unwrap())
}

#[actix_web::get("/metrolinktrackproxy")]
pub async fn metrolinktrackproxy(req: HttpRequest) -> impl Responder {
    let raw_data = reqwest::get("https://rtt.metrolinktrains.com/StationScheduleList.json").await;

    match raw_data {
        Ok(raw_data) => {
            let raw_text = raw_data.text().await;

            match raw_text {
                Ok(raw_text) => HttpResponse::Ok()
                    .insert_header(("Content-Type", "application/json"))
                    .body(raw_text),
                Err(error) => HttpResponse::InternalServerError()
                    .insert_header(("Content-Type", "text/plain"))
                    .body("Could not fetch Metrolink data"),
            }
        }
        Err(error) => HttpResponse::InternalServerError()
            .insert_header(("Content-Type", "text/plain"))
            .body("Could not fetch Metrolink data"),
    }
}

#[actix_web::get("/calfireproxy")]
pub async fn calfireproxy(req: HttpRequest) -> impl Responder {
    let raw_data = reqwest::get(
        "https://incidents.fire.ca.gov/umbraco/api/IncidentApi/GeoJsonList?inactive=false",
    )
    .await;

    match raw_data {
        Ok(raw_data) => {
            let raw_text = raw_data.text().await.unwrap();

            HttpResponse::Ok()
                .insert_header(("Content-Type", "application/json"))
                .body(raw_text)
        }
        Err(err) => HttpResponse::InternalServerError()
            .insert_header(("Content-Type", "text/plain"))
            .body("could not fetch calfire"),
    }
}

#[actix_web::get("/amtrakproxy")]
pub async fn amtrakproxy(req: HttpRequest) -> impl Responder {
    let raw_data =
        reqwest::get("https://maps.amtrak.com/services/MapDataService/trains/getTrainsData").await;

    match raw_data {
        Ok(raw_data) => {
            //println!("Raw data successfully downloaded");

            match amtk::decrypt(raw_data.text().await.unwrap().as_str()) {
                Ok(decrypted_string) => HttpResponse::Ok()
                    .insert_header(("Content-Type", "application/json"))
                    .body(decrypted_string),
                Err(err) => HttpResponse::InternalServerError()
                    .insert_header(("Content-Type", "text/plain"))
                    .body("Could not decrypt Amtrak data"),
            }
        }
        Err(error) => HttpResponse::InternalServerError()
            .insert_header(("Content-Type", "text/plain"))
            .body("Could not fetch Amtrak data"),
    }
}

#[derive(Clone, Debug)]
struct ChateauToSend {
    chateau: String,
    hull: geo::MultiPolygon,
    realtime_feeds: Vec<String>,
    schedule_feeds: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct ChateauToSendNoGeom {
    chateau: String,
    realtime_feeds: Vec<String>,
    schedule_feeds: Vec<String>,
}

#[actix_web::get("/getchateaus")]
async fn chateaus(
    pool: web::Data<Arc<CatenaryPostgresPool>>,
    req: HttpRequest,
    chateau_cache: web::Data<ChateauCacheActixData>,
) -> impl Responder {
    let chateau_lock = chateau_cache.read().unwrap();
    let chateau_as_ref = chateau_lock.as_ref();

    let cloned_chateau_data = chateau_as_ref.cloned();

    drop(chateau_lock);

    if let Some(cloned_chateau_data) = cloned_chateau_data {
        if cloned_chateau_data.last_updated_time_ms
            > SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64
                - 3_600_000
        {
            return HttpResponse::Ok()
                .insert_header(("Content-Type", "application/json"))
                .insert_header(("Cache-Control", "max-age=60, public"))
                .body(cloned_chateau_data.chateau_geojson);
        }
    }

    let conn_pool = pool.as_ref();
    let conn_pre = conn_pool.get().await;
    let conn = &mut conn_pre.unwrap();

    // fetch out of table
    let existing_chateaus = catenary::schema::gtfs::chateaus::table
        .select(catenary::models::Chateau::as_select())
        .load::<catenary::models::Chateau>(conn)
        .await
        .unwrap();

    // convert hulls to standardised `geo` crate
    let mut formatted_chateaus = existing_chateaus
        .into_iter()
        .filter(|pg_chateau| pg_chateau.hull.is_some())
        .map(|pg_chateau| ChateauToSend {
            chateau: pg_chateau.chateau,
            realtime_feeds: pg_chateau.realtime_feeds.into_iter().flatten().collect(),
            schedule_feeds: pg_chateau.static_feeds.into_iter().flatten().collect(),
            hull: diesel_multi_polygon_to_geo(pg_chateau.hull.unwrap()),
        })
        .collect::<Vec<ChateauToSend>>();

    formatted_chateaus.sort_by_key(|x| x.chateau.clone());

    // conversion to `geojson` structs
    let features = formatted_chateaus
        .iter()
        .map(|chateau| {
            let value = geojson::Value::from(&chateau.hull);

            let mut properties: serde_json::map::Map<String, JsonValue> =
                serde_json::map::Map::new();

            properties.insert(
                String::from("chateau"),
                serde_json::Value::String(chateau.chateau.clone()),
            );
            properties.insert(
                String::from("realtime_feeds"),
                serde_json::Value::Array(
                    chateau
                        .realtime_feeds
                        .clone()
                        .into_iter()
                        .map(serde_json::Value::String)
                        .collect(),
                ),
            );
            properties.insert(
                String::from("schedule_feeds"),
                serde_json::Value::Array(
                    chateau
                        .schedule_feeds
                        .clone()
                        .into_iter()
                        .map(serde_json::Value::String)
                        .collect(),
                ),
            );

            geojson::Feature {
                bbox: None,
                geometry: Some(geojson::Geometry {
                    bbox: None,
                    value,
                    foreign_members: None,
                }),
                id: Some(geojson::feature::Id::String(chateau.chateau.clone())),
                properties: Some(properties),
                foreign_members: None,
            }
        })
        .collect::<Vec<Feature>>();

    // formation of final object
    let feature_collection = geojson::FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    };

    // turn it into a string and send it!!!
    let serialized = GeoJson::from(feature_collection).to_string();

    //cache it first
    let mut chateau_lock = chateau_cache.write().unwrap();
    let mut chateau_mut_ref = chateau_lock.as_mut();

    chateau_mut_ref = Some(&mut ChateauCache {
        chateau_geojson: serialized.clone(),
        last_updated_time_ms: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
    });

    drop(chateau_lock);

    HttpResponse::Ok()
        .insert_header(("Content-Type", "application/json"))
        .insert_header(("Cache-Control", "max-age=60,public"))
        .body(serialized)
}

#[actix_web::get("/getchateausnogeom")]
async fn chateaus_no_geom(
    pool: web::Data<Arc<CatenaryPostgresPool>>,
    req: HttpRequest,
) -> impl Responder {
    let conn_pool = pool.as_ref();
    let conn_pre = conn_pool.get().await;
    let conn = &mut conn_pre.unwrap();

    // fetch out of table
    let existing_chateaus = catenary::schema::gtfs::chateaus::table
        .select(catenary::models::Chateau::as_select())
        .load::<catenary::models::Chateau>(conn)
        .await
        .unwrap();

    // convert hulls to standardised `geo` crate
    let mut formatted_chateaus = existing_chateaus
        .into_iter()
        .filter(|pg_chateau| pg_chateau.hull.is_some())
        .map(|pg_chateau| ChateauToSendNoGeom {
            chateau: pg_chateau.chateau,
            realtime_feeds: pg_chateau.realtime_feeds.into_iter().flatten().collect(),
            schedule_feeds: pg_chateau.static_feeds.into_iter().flatten().collect(),
        })
        .collect::<Vec<ChateauToSendNoGeom>>();

    formatted_chateaus.sort_by_key(|x| x.chateau.clone());

    let serialised_chateaus = serde_json::to_string(&formatted_chateaus).unwrap();

    HttpResponse::Ok()
        .insert_header(("Content-Type", "application/json"))
        .insert_header(("Cache-Control", "max-age=60,public"))
        .body(serialised_chateaus)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct IpToGeoApiResp {
    pub data_found: bool,
    pub error: bool,
    pub geo_resp: Option<IpToGeoAddr>,
    pub err_msg: Option<String>,
}

#[actix_web::get("/ip_addr_to_geo/")]
async fn ip_addr_to_geo_api(
    pool: web::Data<Arc<CatenaryPostgresPool>>,
    req: HttpRequest,
) -> impl Responder {
    let connection_info = req.connection_info();

    let resp = match connection_info.realip_remote_addr() {
        None => IpToGeoApiResp {
            data_found: false,
            error: false,
            geo_resp: None,
            err_msg: Some(String::from("No IP found")),
        },
        Some(ip_addr) => {
            let ipaddrparse = ip_addr.parse::<std::net::IpAddr>();

            match ipaddrparse {
                Ok(ipaddrparse) => {
                    let ip_net_cleaned = match ipaddrparse {
                        core::net::IpAddr::V4(ip_addr_v4) => {
                            ipnet::IpNet::new(ipaddrparse, 32).unwrap()
                        }
                        core::net::IpAddr::V6(ip_addr_v6) => {
                            ipnet::IpNet::new(ipaddrparse, 128).unwrap()
                        }
                    };

                    let pg_lookup = catenary::ip_to_location::lookup_geo_from_ip_addr(
                        Arc::clone(&pool.into_inner()),
                        ip_net_cleaned,
                    )
                    .await;

                    match pg_lookup {
                        Err(err_a) => {
                            eprintln!("{:#?}", err_a);
                            IpToGeoApiResp {
                                data_found: false,
                                error: true,
                                geo_resp: None,
                                err_msg: Some(String::from("Lookup error")),
                            }
                        }
                        Ok(pg_lookup) => match pg_lookup.len() {
                            0 => IpToGeoApiResp {
                                data_found: false,
                                error: false,
                                geo_resp: None,
                                err_msg: Some(String::from("no rows found")),
                            },
                            _ => IpToGeoApiResp {
                                data_found: true,
                                error: false,
                                geo_resp: Some(pg_lookup[0].clone()),
                                err_msg: None,
                            },
                        },
                    }
                }
                Err(ip_destructure_err) => {
                    eprintln!(
                        "UNABLE TO GET IP ADDRESS from user {:#?}, {}",
                        ip_destructure_err, ip_addr
                    );

                    IpToGeoApiResp {
                        data_found: false,
                        error: true,
                        geo_resp: None,
                        err_msg: Some(String::from("UNABLE TO GET IP ADDRESS from user")),
                    }
                }
            }
        }
    };

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-cache"))
        .json(resp)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // std::env::set_var("RUST_LOG", "debug");
    // env_logger::init();

    // Connect to the database.
    let pool = Arc::new(make_async_pool().await.unwrap());
    let arc_pool = Arc::clone(&pool);

    //let conn_pre = arc_pool.as_ref().get().await;
    // let conn = &mut conn_pre.unwrap();

    let sqlx_pool: Arc<sqlx::Pool<sqlx::Postgres>> = Arc::new(
        PgPoolOptions::new()
            .max_connections(16)
            .connect(std::env::var("DATABASE_URL").unwrap().as_str())
            .await
            .unwrap(),
    );

    let etcd_urls_original =
        std::env::var("ETCD_URLS").unwrap_or_else(|_| "localhost:2379".to_string());
    let etcd_urls = etcd_urls_original
        .split(',')
        .map(|x| x.to_string())
        .collect::<Vec<String>>();

    let etcd_connection_ips = Arc::new(EtcdConnectionIps {
        ip_addresses: etcd_urls,
    });

    let etcd_username = std::env::var("ETCD_USERNAME");

    let etcd_password = std::env::var("ETCD_PASSWORD");

    let etcd_connection_options: Option<etcd_client::ConnectOptions> =
        match (etcd_username, etcd_password) {
            (Ok(username), Ok(password)) => {
                Some(etcd_client::ConnectOptions::new().with_user(username, password))
            }
            _ => None,
        };

    // Create a new HTTP server.
    let builder = HttpServer::new(move || {
        App::new()
            .wrap(
                DefaultHeaders::new()
                    .add(("Access-Control-Allow-Origin", "*"))
                    .add(("Server", "Catenary"))
                    .add((
                        "Access-Control-Allow-Origin",
                        "https://maps.catenarymaps.org",
                    )),
            )
            .wrap(actix_block_ai_crawling::BlockAi)
            .wrap(middleware::Compress::default())
            .app_data(actix_web::web::Data::new(Arc::clone(&sqlx_pool)))
            .app_data(actix_web::web::Data::new(Arc::clone(&pool)))
            .app_data(actix_web::web::Data::new(Arc::new(RwLock::new(
                None::<ChateauCache>,
            ))))
            .app_data(actix_web::web::Data::new(Arc::new(
                etcd_connection_options.clone(),
            )))
            .app_data(actix_web::web::Data::new(Arc::clone(&etcd_connection_ips)))
            .route("/", web::get().to(index))
            .route("robots.txt", web::get().to(robots))
            .service(amtrakproxy)
            .service(microtime)
            .service(nanotime)
            .service(chateaus)
            .service(metrolinktrackproxy)
            .service(shapes_not_bus)
            .service(shapes_not_bus_meta)
            .service(shapes_bus)
            .service(shapes_bus_meta)
            .service(routesofchateau)
            .service(bus_stops_meta)
            .service(bus_stops)
            .service(rail_stops)
            .service(rail_stops_meta)
            .service(station_features)
            .service(station_features_meta)
            .service(other_stops)
            .service(other_stops_meta)
            .service(chateaus_no_geom)
            .service(api_key_management::get_realtime_keys)
            .service(api_key_management::set_realtime_key)
            .service(aspenised_data_over_https::get_realtime_locations)
            .service(chicago_proxy::ttarrivals_proxy)
            .service(nearby_departures::nearby_from_coords)
            .service(departures_at_stop::departures_at_stop)
            .service(get_vehicle_trip_information::get_trip_init)
            .service(get_vehicle_trip_information::get_trip_rt_update)
            .service(get_vehicle_trip_information::get_vehicle_information)
            .service(get_vehicle_trip_information::get_vehicle_information_from_label)
            .service(calfireproxy)
            .service(ip_addr_to_geo_api)
            .service(route_info::route_info)
            .service(gtfs_rt_api::gtfs_rt)
            .service(shapes_local_rail)
            .service(shapes_local_rail_meta)
            .service(shapes_intercity_rail)
            .service(shapes_intercity_rail_meta)
            .service(shapes_ferry)
            .service(shapes_ferry_meta)
            .service(get_agencies::get_agencies_raw)
    })
    .workers(16);

    let _ = builder.bind("127.0.0.1:17419").unwrap().run().await;

    Ok(())
}
