use devdb::{
    dev_db_server::{DevDb, DevDbServer},
    info_entry, DeviceInfo, DeviceInfoReply, DeviceList, InfoEntry, Property,
};
use futures::future;
use hyper::{service::make_service_fn, Server};
use std::convert::Infallible;
use tonic::{Request, Response, Status};
use tower::Service;
use tracing::{error, info, Level};

pub mod devdb {
    tonic::include_proto!("devdb");
}

// Create an empty type to associate it with the gRPC handlers.

#[derive(Default)]
pub struct DevDB {}

#[tonic::async_trait]
impl DevDb for DevDB {
    async fn get_device_info(
        &self,
        request: Request<DeviceList>,
    ) -> Result<Response<DeviceInfoReply>, Status> {
        info!("request for {:?}", request.get_ref().device);

        Ok(Response::new(DeviceInfoReply {
            set: request
                .get_ref()
                .device
                .iter()
                .map(|dev| InfoEntry {
                    name: dev.clone(),
                    result: Some(info_entry::Result::Device(DeviceInfo {
                        index: 0,
                        reading: Some(Property {
                            primary_units: "V".into(),
                            common_units: "mm".into(),
                        }),
                        setting: Some(Property {
                            primary_units: "V".into(),
                            common_units: "mm".into(),
                        }),
                        description: "description".into(),
                    })),
                })
                // Pull all entries from the iterator and store in a vector.
                .collect::<Vec<InfoEntry>>(),
        }))
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

    let grpc_server = DevDbServer::new(DevDB::default());

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
