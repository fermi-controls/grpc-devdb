use futures::{Stream, StreamExt};
use proto::{
    dev_db_server::{DevDb, DevDbServer},
    info_entry, DeviceInfo, DeviceInfoReply, DeviceList, DigitalControl, DigitalControlItem,
    InfoEntry, Property,
};
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::pin::Pin;
use tonic::{transport::Server, Request, Response, Status};
use tracing::{error, info, info_span, warn, Level};
use tracing_futures::Instrument;

// Place all the Protobuf stuff in a `proto` module.

mod proto {
    tonic::include_proto!("devdb");

    pub(crate) const FILE_DESCRIPTOR_SET: &[u8] =
        tonic::include_file_descriptor_set!("devdb_descriptor");
}

// The gRPC hander for this API needs to access the database. So the
// global state used by the service will hold a pool of connections.

struct DevDB {
    pool: PgPool,
}

// This defines the row (with types) that we expect from our
// query. This structure should be kept in sync with the actual query
// (otherwise we'll get runtime errors.)

#[derive(sqlx::FromRow, Debug)]
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

    const BC_QUERY: &str = r#"
SELECT P.value as val,
       CAST(P.short_name AS TEXT) as sname,
       CAST(P.long_name AS TEXT) as lname
  FROM accdb.device D
    JOIN accdb.digital_control P USING(di)
  WHERE D.name = $1
  ORDER BY order_number"#;

    async fn query_device(&self, item: &str) -> InfoEntry {
        // Build and prep the SQL query for this iteration.

        let mut sql_cmd: Fetch<RowInfo> = sqlx::query_as(DevDB::QUERY).bind(item).fetch(&self.pool);

        // Local copies of the device info that we're accumulating.

        let mut index: u32 = 0;
        let mut description: String = "".into();
        let mut r_prop: Option<Property> = None;
        let mut s_prop: Option<Property> = None;

        // Loop through the database results.

        while let Some(row) = sql_cmd.next().await {
            match row {
                Ok(row) => {
                    index = row.di as u32;
                    description = row.descr.clone();

                    // Build a property type.

                    let prop = Property {
                        primary_units: Some(row.p_units.clone()),
                        common_units: Some(row.c_units.clone()),
                    };

                    // Now fill in the appropriate property. 12 is
                    // for readings and 13 is for settings. Our
                    // query only returns these two properties.

                    if row.pi == 12 {
                        r_prop = Some(prop)
                    } else {
                        s_prop = Some(prop)
                    }
                }
                Err(e) => {
                    warn!("couldn't decode row for {} : {}", &item, &e);
                    let tmp = InfoEntry {
                        name: item.into(),
                        result: Some(info_entry::Result::ErrMsg(format!("{}", e))),
                    };

                    return tmp;
                }
            }
        }

        // Now look for digital control information.

        let mut cmds = vec![];

        {
            let mut sql_cmd: Fetch<(i32, String, String)> =
                sqlx::query_as(DevDB::BC_QUERY).bind(item).fetch(&self.pool);

            while let Some(row) = sql_cmd.next().await {
                match row {
                    Ok((value, short_name, long_name)) => cmds.push(DigitalControlItem {
                        value: value as u32,
                        short_name,
                        long_name,
                    }),
                    Err(e) => {
                        error!("couldn't parse digital status : {}", e);
                        cmds.clear();
                        break;
                    }
                }
            }
        }

        // Build final value.

        InfoEntry {
            name: item.into(),
            result: Some(info_entry::Result::Device(DeviceInfo {
                index,
                description,
                reading: r_prop,
                setting: s_prop,
                dig_control: if cmds.is_empty() {
                    None
                } else {
                    Some(DigitalControl { cmds })
                },
            })),
        }
    }
}

type Fetch<'a, T> = Pin<Box<dyn Stream<Item = Result<T, sqlx::Error>> + Send + 'a>>;

#[tonic::async_trait]
impl DevDb for DevDB {
    async fn get_device_info(
        &self,
        request: Request<DeviceList>,
    ) -> Result<Response<DeviceInfoReply>, Status> {
        let client = request
            .remote_addr()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unknown".into());

        async {
            info!("devices {:?}", request.get_ref().device);

            let mut result = vec![];

            // Loop through each device.

            for item in &request.get_ref().device {
                result.push(self.query_device(item).await)
            }

            Ok(Response::new(DeviceInfoReply { set: result }))
        }
        .instrument(info_span!("dev-info", client))
        .await
    }
}

fn setup_logging() {
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_target(false)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("unable to initialize trace facility");
}

#[tokio::main]
async fn main() {
    setup_logging();

    // Define the address for the gRPC service to use.

    let addr = "0.0.0.0:6802".parse().unwrap();

    // Create a pool of connections to PostgreSQL. We start with a
    // pool of 5 connections.

    let pool_fut = PgPoolOptions::new()
        .max_connections(5)
        .connect("postgres://guest:GUEST1@dbsrv.fnal.gov/adbs");

    match pool_fut.await {
        Ok(pool) => {
            let refl_service = tonic_reflection::server::Builder::configure()
                .register_encoded_file_descriptor_set(proto::FILE_DESCRIPTOR_SET)
                .build()
                .unwrap();

            // Move the connection pool into the state of our gRPC
            // service.

            let grpc_server = DevDbServer::new(DevDB { pool });

            let _ = Server::builder()
                .add_service(refl_service)
                .add_service(grpc_server)
                .serve(addr)
                .await;
        }
        Err(e) => error!("{}", e),
    }
}
