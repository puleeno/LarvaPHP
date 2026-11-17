//! Caddy module handlers.
//!
//! Mirrors `caddy/module.go` from the Go source.

use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use larva_php::errors::Result;
use larva_php::options::RequestOption;

use crate::LarvaPHPModule;

impl LarvaPHPModule {
    /// Serve an HTTP request.
    pub async fn serve_http(
        &self,
        _req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<hyper::Response<http_body_util::combinators::BoxBody<Bytes, std::convert::Infallible>>> {
        // TODO: implement full request handling
        Ok(hyper::Response::builder()
            .status(200)
            .body(Empty::<Bytes>::new().boxed())
            .unwrap())
    }

    /// Get request options for this module.
    pub fn request_options(&self) -> Vec<Box<dyn RequestOption>> {
        let mut opts: Vec<Box<dyn RequestOption>> = Vec::new();

        opts.push(Box::new(larva_php::options::WithRequestSplitPath(
            self.split_path.clone(),
        )));

        if let Some(timeout) = self.request_body_timeout {
            opts.push(Box::new(larva_php::options::WithRequestBodyTimeout(timeout)));
        }

        if !self.resolved_env.is_empty() {
            opts.push(Box::new(larva_php::options::WithRequestEnv(
                self.resolved_env.clone(),
            )));
        }

        opts
    }
}