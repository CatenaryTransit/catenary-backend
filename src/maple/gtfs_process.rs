use postgis::ewkb;
use rgb::RGB;
use std::collections::HashMap;
use std::collections::HashSet;
use std::error::Error;
use std::sync::Arc;

use crate::gtfs_handlers::colour_correction;
use crate::gtfs_handlers::enum_to_int::route_type_to_int;
use crate::gtfs_handlers::rename_route_labels::*;
use crate::gtfs_handlers::shape_colour_calculator::shape_to_colour;
use crate::gtfs_handlers::stops_associated_items::*;
use crate::gtfs_ingestion_sequence::shapes_into_postgres::shapes_into_postgres;

// Initial version 3 of ingest written by Kyler Chin
// Removal of the attribution is not allowed, as covered under the AGPL license

// take a feed id and throw it into postgres
pub async fn gtfs_process_feed(
    feed_id: &str,
    pool: &Arc<sqlx::Pool<sqlx::Postgres>>,
) -> Result<(), Box<dyn Error>> {
    let path = format!("gtfs_uncompressed/{}", feed_id);

    let gtfs = gtfs_structures::Gtfs::new(path.as_str())?;

    let (stop_ids_to_route_types, stop_ids_to_route_ids) =
        make_hashmap_stops_to_route_types_and_ids(&gtfs);

    let (stop_id_to_children_ids, stop_ids_to_children_route_types) =
        make_hashmaps_of_children_stop_info(&gtfs, &stop_ids_to_route_types);

    //identify colours of shapes based on trip id's route id
    let (shape_to_color_lookup, shape_to_text_color_lookup) = shape_to_colour(&feed_id, &gtfs);

    //shove raw geometry into postgresql
    shapes_into_postgres(&gtfs, &shape_to_color_lookup, &shape_to_text_color_lookup, &feed_id, pool).await?;

    Ok(())
}
