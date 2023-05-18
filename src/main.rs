use devdb::{
    dev_db_server::{DevDb, DevDbServer},
    info_entry, DeviceInfo, DeviceInfoReply, DeviceList, InfoEntry, Property,
};
use futures::{future, Stream, StreamExt};
use hyper::{service::make_service_fn, Server};
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::{convert::Infallible, pin::Pin};
use tonic::{Request, Response, Status};
use tower::Service;
use tracing::{error, info, Level};

pub mod devdb {
    tonic::include_proto!("devdb");
}

// Create an empty type to associate it with the gRPC handlers.

pub struct DevDB {
    pub pool: PgPool,
}

#[derive(sqlx::FromRow)]
struct RowInfo {
    di: i32,
    pi: i32,
    descr: String,
    p_units: String,
    c_units: String,
}

impl DevDB {
    const QUERY: &str = r#"
SELECT di,
       pi,
       CAST (D.description AS TEXT) AS descr,
       CAST (S.primary_text AS TEXT) AS p_units,
       CAST (S.common_text AS TEXT) AS c_units
  FROM accdb.device D
    JOIN accdb.property P USING(di)
    JOIN accdb.device_scaling S USING(di, pi)
  WHERE D.name = $1 and pi in (12, 13)"#;
}

type Fetch<'a, T> = Pin<Box<dyn Stream<Item = Result<T, sqlx::Error>> + Send + 'a>>;

#[tonic::async_trait]
impl DevDb for DevDB {
    async fn get_device_info(
        &self,
        request: Request<DeviceList>,
    ) -> Result<Response<DeviceInfoReply>, Status> {
        info!("request for {:?}", request.get_ref().device);

        let mut result = vec![];

        // Loop through each device.

        'outer: for item in &request.get_ref().device {
            // Build and prep the SQL query for this iteration.

            let mut sql_cmd: Fetch<RowInfo> =
                sqlx::query_as(DevDB::QUERY).bind(item).fetch(&self.pool);

            // Local copies of the device info that we're accumulating.

            let mut index: u32 = 0;
            let mut description: String = "".into();
            let mut r_prop: Option<Property> = None;
            let mut s_prop: Option<Property> = None;

            // Loop through the database results.

            while let Some(row) = sql_cmd.next().await {
                if let Ok(row) = row {
                    index = row.di as u32;
                    description = row.descr.clone();

                    // Build a property type.

                    let prop = Property {
                        primary_units: Some(row.p_units.clone()),
                        common_units: Some(row.c_units.clone()),
                    };

                    // Now fill in the appropriate propery. 12 is for
                    // readings and 13 is for settings. Our query only
                    // returns these two properties.

                    if row.pi == 12 {
                        r_prop = Some(prop)
                    } else {
                        s_prop = Some(prop)
                    }
                } else {
                    break 'outer;
                }
            }

            result.push(InfoEntry {
                name: item.into(),
                result: Some(info_entry::Result::Device(DeviceInfo {
                    index,
                    description,
                    reading: r_prop,
                    setting: s_prop,
                })),
            });
        }

        Ok(Response::new(DeviceInfoReply { set: result }))
    }
}

#[tokio::main]
async fn main() {
    let addr = "0.0.0.0:50051".parse().unwrap();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_target(false)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("unable to initialize trace facility");

    let pool_fut = PgPoolOptions::new()
        .max_connections(5)
        .connect("postgres://guest:GUEST1@dbsrv.fnal.gov/adbs");

    match pool_fut.await {
        Ok(pool) => {
            let grpc_server = DevDbServer::new(DevDB { pool });
            let http_server = Server::bind(&addr).serve(make_service_fn(move |_| {
                let mut grpc_server = grpc_server.clone();

                future::ok::<_, Infallible>(tower::service_fn(
                    move |req: hyper::Request<hyper::Body>| grpc_server.call(req),
                ))
            }));

            if let Err(e) = http_server.await {
                error!("httpd error: {}", e);
            }
        }
        Err(e) => error!("{}", e),
    }
}
