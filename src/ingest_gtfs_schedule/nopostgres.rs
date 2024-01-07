use serde_json::Error as SerdeError;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fs;
mod dmfr;
use futures;
use gtfs_structures::RouteType;
use rgb::RGB;
use std::error::Error;
use tokio::runtime;

extern crate fs_extra;
use fs_extra::dir::get_size;

pub fn path_exists(path: &str) -> bool {
    fs::metadata(path).is_ok()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let postgresstring = arguments::parse(std::env::args())
        .unwrap()
        .get::<String>("postgres");

    let threads = arguments::parse(std::env::args())
        .unwrap()
        .get::<usize>("threads");

    let threadcount = threads.unwrap();

    let postgresstring = match postgresstring {
        Some(s) => s,
        None => {
            panic!("You need a postgres string");
        }
    };

    if let Ok(entries) = fs::read_dir("transitland-atlas/feeds") {
        let mut feedhashmap: BTreeMap<String, dmfr::Feed> = BTreeMap::new();

        let mut operatorhashmap: BTreeMap<String, dmfr::Operator> = BTreeMap::new();

        let mut operator_to_feed_hashmap: BTreeMap<String, Vec<dmfr::OperatorAssociatedFeedsItem>> =
            BTreeMap::new();

        let mut feed_to_operator_hashmap: BTreeMap<String, Vec<String>> = BTreeMap::new();

        for entry in entries {
            if let Ok(entry) = entry {
                if let Some(file_name) = entry.file_name().to_str() {
                    println!("{}", file_name);

                    let contents =
                        fs::read_to_string(format!("transitland-atlas/feeds/{}", file_name));

                    match contents {
                        Ok(contents) => {
                            let dmfrinfo: Result<
                                dmfr::DistributedMobilityFeedRegistry,
                                SerdeError,
                            > = serde_json::from_str(&contents);

                            match dmfrinfo {
                                Ok(dmfrinfo) => {
                                    dmfrinfo.feeds.iter().for_each(|feed| {
                                        for eachoperator in feed.operators.clone().into_iter() {
                                            if feed_to_operator_hashmap.contains_key(&feed.id) {
                                                feed_to_operator_hashmap.insert(
                                                    feed.id.clone(),
                                                    feed_to_operator_hashmap
                                                        .get(&feed.id)
                                                        .unwrap()
                                                        .clone()
                                                        .into_iter()
                                                        .chain(vec![eachoperator
                                                            .onestop_id
                                                            .clone()])
                                                        .collect::<Vec<String>>(),
                                                );
                                            } else {
                                                feed_to_operator_hashmap.insert(
                                                    feed.id.clone(),
                                                    vec![eachoperator.onestop_id.clone()],
                                                );
                                            }
                                        }

                                        //println!("Feed {}: {:#?}", feed.id.clone(), feed);

                                        if !feedhashmap.contains_key(&feed.id) {
                                            //feedhashmap.insert(feed.id.clone(), feed.clone());
                                            feedhashmap.insert(feed.id.clone(), feed.clone());
                                        }

                                        feed.operators.iter().for_each(|operator| {
                                            operatorhashmap.insert(
                                                operator.onestop_id.clone(),
                                                operator.clone(),
                                            );

                                            if operator_to_feed_hashmap
                                                .contains_key(&operator.onestop_id)
                                            {
                                                //combine the feeds for this operator together
                                                let mut existing_associated_feeds =
                                                    operator_to_feed_hashmap
                                                        .get(&operator.onestop_id)
                                                        .unwrap()
                                                        .clone();

                                                let existing_feed_ids = operator_to_feed_hashmap
                                                    .get(&operator.onestop_id)
                                                    .unwrap()
                                                    .iter()
                                                    .map(|associated_feed| {
                                                        associated_feed
                                                            .feed_onestop_id
                                                            .clone()
                                                            .unwrap()
                                                    })
                                                    .collect::<Vec<String>>();

                                                operator.associated_feeds.iter().for_each(
                                                    |associated_feed| {
                                                        if !existing_feed_ids.contains(
                                                            &associated_feed
                                                                .feed_onestop_id
                                                                .clone()
                                                                .unwrap_or_else(|| feed.id.clone()),
                                                        ) {
                                                            existing_associated_feeds
                                                                .push(associated_feed.clone());
                                                        }
                                                    },
                                                );

                                                operator_to_feed_hashmap.insert(
                                                    operator.onestop_id.clone(),
                                                    existing_associated_feeds,
                                                );
                                            } else {
                                                operator_to_feed_hashmap.insert(
                                                    operator.onestop_id.clone(),
                                                    operator.associated_feeds.clone(),
                                                );
                                            }
                                        });
                                    });

                                    dmfrinfo.operators.iter().for_each(|operator| {
                                        operatorhashmap
                                            .insert(operator.onestop_id.clone(), operator.clone());

                                        println!(
                                            "Operator {}: {:?}",
                                            operator.onestop_id.clone(),
                                            operator.associated_feeds
                                        );

                                        for feed in operator.associated_feeds.iter() {
                                            if feed.feed_onestop_id.is_some() {
                                                if feed_to_operator_hashmap.contains_key(
                                                    feed.feed_onestop_id.as_ref().unwrap().as_str(),
                                                ) {
                                                    feed_to_operator_hashmap.insert(
                                                        feed.feed_onestop_id.clone().unwrap(),
                                                        feed_to_operator_hashmap
                                                            .get(
                                                                feed.feed_onestop_id
                                                                    .as_ref()
                                                                    .unwrap()
                                                                    .as_str(),
                                                            )
                                                            .unwrap()
                                                            .clone()
                                                            .into_iter()
                                                            .chain(vec![operator
                                                                .onestop_id
                                                                .clone()])
                                                            .collect::<Vec<String>>(),
                                                    );
                                                } else {
                                                    feed_to_operator_hashmap.insert(
                                                        feed.feed_onestop_id.clone().unwrap(),
                                                        vec![operator.onestop_id.clone()],
                                                    );
                                                }
                                            }
                                        }

                                        if operator_to_feed_hashmap
                                            .contains_key(&operator.onestop_id)
                                        {
                                            //combine the feeds for this operator together
                                            let mut existing_associated_feeds =
                                                operator_to_feed_hashmap
                                                    .get(&operator.onestop_id)
                                                    .unwrap()
                                                    .clone();

                                            let existing_feed_ids = operator_to_feed_hashmap
                                                .get(&operator.onestop_id)
                                                .unwrap()
                                                .iter()
                                                .filter(|associated_feed| {
                                                    associated_feed.feed_onestop_id.is_some()
                                                })
                                                .map(|associated_feed| {
                                                    associated_feed.feed_onestop_id.clone().unwrap()
                                                })
                                                .collect::<Vec<String>>();

                                            operator.associated_feeds.iter().for_each(
                                                |associated_feed| {
                                                    if !existing_feed_ids.contains(
                                                        &associated_feed
                                                            .feed_onestop_id
                                                            .clone()
                                                            .unwrap(),
                                                    ) {
                                                        existing_associated_feeds
                                                            .push(associated_feed.clone());
                                                    }
                                                },
                                            );

                                            operator_to_feed_hashmap.insert(
                                                operator.onestop_id.clone(),
                                                existing_associated_feeds,
                                            );
                                        } else {
                                            operator_to_feed_hashmap.insert(
                                                operator.onestop_id.clone(),
                                                operator.associated_feeds.clone(),
                                            );
                                        }
                                    });
                                }
                                Err(e) => {}
                            }
                        }
                        Err(e) => {}
                    }
                }
            }
        }

        //let threadpool = ThreadPool::new(threadcount);

        let threaded_rt = runtime::Builder::new_multi_thread()
            .worker_threads(threadcount)
            .enable_all()
            .build()
            .unwrap();

        let mut handles = vec![];

        for (key, feed) in feedhashmap.clone().into_iter() {
            handles.push(threaded_rt.spawn(async move {
                //println!("Feed in future {}: {:#?}", key, feed);

                let mut dothetask = true;

                if key.contains("~jp")
                    || key.contains("germany~urban~transport")
                    || key.contains("~gov~uk")
                {
                    dothetask = false;
                }

                if dothetask {
                    match feed.spec {
                        dmfr::FeedSpec::Gtfs => {
                            //println!("{:?}", feed.urls);

                            if feed.urls.static_current.is_some() {
                                //check if folder exists in the directory

                                //process and upload routes, stops, headways, and shapes etc into postgres

                                //calculate the bounds of the feed,

                                //upload the feed id metadata

                                let file_path = format!("gtfs_uncompressed/{}/", key);

                                if path_exists(&file_path) {
                                    //feed exists

                                    println!("Starting read for {}", &key);

                                    let folder_size = get_size(&file_path).unwrap();
                                    println!("size: {} kB", folder_size / 1000);

                                    let gtfs = gtfs_structures::GtfsReader::default()
                                        .read_from_path(&file_path);

                                    if false {
                                        let gtfs = gtfs.unwrap();

                                        println!("read_duration: {:?}ms", gtfs.read_duration);

                                        println!(
                                            "there are {} stops in the gtfs",
                                            gtfs.stops.len()
                                        );

                                        println!(
                                            "there are {} routes in the gtfs",
                                            gtfs.routes.len()
                                        );

                                        let mut least_lat: Option<f64> = None;
                                        let mut least_lon: Option<f64> = None;

                                        let mut most_lat: Option<f64> = None;
                                        let mut most_lon: Option<f64> = None;

                                        let timestarting = std::time::Instant::now();

                                        let mut shapes_per_route: HashMap<String, Vec<String>> =
                                            HashMap::new();

                                        let mut shape_to_color_lookup: BTreeMap<String, RGB<u8>> =
                                            BTreeMap::new();

                                        for (trip_id, trip) in &gtfs.trips {
                                            if trip.shape_id.is_some() {
                                                if !shape_to_color_lookup.contains_key(
                                                    &trip.shape_id.as_ref().unwrap().clone(),
                                                ) {
                                                    if gtfs.routes.contains_key(&trip.route_id) {
                                                        let route = gtfs
                                                            .routes
                                                            .get(&trip.route_id)
                                                            .unwrap();

                                                        let color = route.color.clone();

                                                        shape_to_color_lookup.insert(
                                                            trip.shape_id.as_ref().unwrap().clone(),
                                                            color,
                                                        );
                                                    }
                                                }
                                            }
                                        }

                                        //let prepared_shapes = client.prepare("INSERT INTO gtfs.shapes (onestop_feed_id, shape_id, linestring, color, routes) VALUES ($1, $2, $3, $4, $5);").await.unwrap();

                                        for (shape_id, shape) in &gtfs.shapes {
                                            let color_to_upload =
                                                match shape_to_color_lookup.get(shape_id) {
                                                    Some(color) => format!(
                                                        "{:02x}{:02x}{:02x}",
                                                        color.r, color.g, color.b
                                                    ),
                                                    None => String::from("3a3a3a"),
                                                };

                                            //bug "Line String must at least have 2 points"

                                            /*
                                              CREATE TABLE IF NOT EXISTS gtfs.shapes (
                                                    onestop_feed_id text NOT NULL,
                                                    shape_id text NOT NULL,
                                                    linestring GEOMETRY(LINESTRING,4326) NOT NULL,
                                                    color text,
                                                    PRIMARY KEY (onestop_feed_id,shape_id)
                                                );
                                            */

                                            // println!("uploading shape {:?} {:?}", &feed.id, &shape_id);
                                        }

                                        for (route_id, route) in &gtfs.routes {
                                            let route_type_number = match &route.route_type {
                                                RouteType::Tramway => 0,
                                                RouteType::Subway => 1,
                                                RouteType::Rail => 2,
                                                RouteType::Bus => 3,
                                                RouteType::Ferry => 4,
                                                RouteType::CableCar => 5,
                                                RouteType::Gondola => 6,
                                                RouteType::Funicular => 7,
                                                RouteType::Coach => 200,
                                                RouteType::Air => 1100,
                                                RouteType::Taxi => 1500,
                                                RouteType::Other(i) => *i,
                                            };

                                            let mut shape_id_array: Vec<String> =
                                                match shapes_per_route.get(route_id) {
                                                    Some(shape_list) => shape_list.clone(),
                                                    None => vec![],
                                                };

                                            shape_id_array.dedup();

                                            let shape_id_array = shape_id_array;

                                            //println!("uploading route {:?} {}", &feed.id , &route_id);
                                        }

                                        println!("Uploading {} trips", gtfs.trips.len());

                                        let time = std::time::Instant::now();

                                        // let statement = client.prepare("INSERT INTO gtfs.trips (onestop_feed_id, trip_id, service_id, route_id, trip_headsign, trip_short_name, shape_id) VALUES ($1, $2, $3, $4, $5, $6, $7);").await.unwrap();

                                        /*


                                            let mut threaded_trips = runtime::Builder::new_multi_thread()
                                        .worker_threads(5)
                                        .enable_time()
                                        .build()
                                        .unwrap();

                                        let mut trips_handles = vec![];

                                        for (trip_id, trip) in gtfs.trips.clone().into_iter() {
                                            let pool = pool.clone();

                                            let feed_id = feed.id.clone();
                                            trips_handles.push(threaded_trips.spawn(


                                                async move {

                                                    println!("Uploading trip {}", &trip.id);

                                                let mut client = pool.get().await.unwrap();



                                                   client
                                                        .query(
                                                            "INSERT INTO gtfs.trips (onestop_feed_id, trip_id, service_id, route_id, trip_headsign, trip_short_name) VALUES ($1, $2, $3, $4, $5, $6);",
                                                            &[
                                                                &feed_id,
                                                                   &trip.id,
                                                                 &trip.service_id,
                                             &trip.route_id,
                                                  &trip.trip_headsign.unwrap_or_else(|| "".to_string()),
                                                          &trip.trip_short_name.unwrap_or_else(|| "".to_string()),
                                                               ],
                                                        ).await.unwrap();
                                                }

                                            ));
                                        }

                                        let time = std::time::Instant::now();

                                        futures::future::join_all(trips_handles).await;
                                        println!("{} with {} trips took {}ms", feed.id, gtfs.trips.len(), time.elapsed().as_millis());

                                         */

                                        /*




                                        let trips_insertion_multithread = futures::stream::iter(gtfs.trips.clone().into_iter().map(|(trip_id, trip)| {

                                            let pool = pool.clone();

                                            let feed_id = feed.id.clone();

                                            async move {

                                            let mut client = pool.get().await.unwrap();



                                               client
                                                    .query(
                                                        "INSERT INTO gtfs.trips (onestop_feed_id, trip_id, service_id, route_id, trip_headsign, trip_short_name) VALUES ($1, $2, $3, $4, $5, $6);",
                                                        &[
                                                            &feed_id,
                                                            &trip.id,
                                                            &trip.service_id,
                                                            &trip.route_id,
                                                            &trip.trip_headsign.unwrap_or_else(|| "".to_string()),
                                                            &trip.trip_short_name.unwrap_or_else(|| "".to_string()),
                                                           ],
                                                    ).await.unwrap();
                                            }
                                        }))
                                        .buffer_unordered(1)
                                        .collect::<Vec<()>>();

                                        trips_insertion_multithread.await;
                                         */

                                        //okay finally upload the feed metadata

                                        /*

                                                                onestop_feed_id text PRIMARY KEY,
                                        onestop_operator_id text,
                                        gtfs_agency_id text,
                                        name text ,
                                        url text ,
                                        timezone text,
                                        lang text,
                                        phone text,
                                        fare_url text,
                                        email text,
                                        max_lat double precision NOT NULL,
                                        max_lon double precision NOT NULL,
                                        min_lat double precision NOT NULL,
                                        min_lon double precision NOT NULL
                                         */

                                        if gtfs.routes.len() > 0 as usize {}
                                    }
                                }
                            }
                        }
                        dmfr::FeedSpec::GtfsRt => {}
                        _ => {
                            //do nothing
                            println!("skipping {}, does not match dmfr feed spec", &key);
                        }
                    }
                }
            }));
        }

        futures::future::join_all(handles).await;

        println!("Done ingesting all gtfs!");
    }

    Ok(())
}
