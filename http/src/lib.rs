wasmtime::component::bindgen!({
    path: "../wit",
    interfaces: "import wasi:http/types@0.3.0-draft-2024-02-14;",
    isyswasfa: true,
    with: {
        "wasi:io/error": wasmtime_wasi::preview2::bindings::wasi::io::error,
        "wasi:io/streams": wasmtime_wasi::preview2::bindings::wasi::io::streams,
        "wasi:http/types/request": Request,
        "wasi:http/types/request-options": RequestOptions,
        "wasi:http/types/response": Response,
        "wasi:http/types/fields": Fields,
        "wasi:http/types/isyswasfa-receiver-own-trailers": FieldsReceiver,
        "wasi:http/types/isyswasfa-sender-own-trailers": FieldsSender,
    }
});

use {
    anyhow::anyhow,
    async_trait::async_trait,
    futures::channel::oneshot,
    isyswasfa_host::IsyswasfaView,
    std::sync::MutexGuard,
    wasi::http::types::{ErrorCode, HeaderError, Method, RequestOptionsError, Scheme},
    wasmtime::component::{Linker, Resource, ResourceTable},
    wasmtime_wasi::preview2::{bindings::wasi::io::error::Error, InputStream},
};

pub trait WasiHttpView: Send + IsyswasfaView + 'static {
    fn table(&mut self) -> &mut ResourceTable;
    fn shared_table(&self) -> MutexGuard<ResourceTable>;
}

pub trait WasiHttpState: Send + 'static {
    fn shared_table(&self) -> MutexGuard<ResourceTable>;
}

#[derive(Clone)]
pub struct Fields(pub Vec<(String, Vec<u8>)>);

pub struct FieldsSender(pub oneshot::Sender<Fields>);

pub struct FieldsReceiver(pub oneshot::Receiver<Fields>);

#[derive(Default, Copy, Clone)]
pub struct RequestOptions {
    pub connect_timeout: Option<u64>,
    pub first_byte_timeout: Option<u64>,
    pub between_bytes_timeout: Option<u64>,
}

pub struct Request {
    pub method: Method,
    pub scheme: Option<Scheme>,
    pub path_with_query: Option<String>,
    pub authority: Option<String>,
    pub headers: Fields,
    pub body: Option<InputStream>,
    pub trailers: Option<FieldsReceiver>,
    pub options: Option<RequestOptions>,
}

pub struct Response {
    pub status_code: u16,
    pub headers: Fields,
    pub body: Option<InputStream>,
    pub trailers: Option<FieldsReceiver>,
}

impl<T: WasiHttpView> wasi::http::types::HostIsyswasfaSenderOwnTrailers for T {
    fn send(
        &mut self,
        this: Resource<FieldsSender>,
        fields: Resource<Fields>,
    ) -> wasmtime::Result<()> {
        let (sender, fields) = {
            let mut table = self.shared_table();
            let sender = table.delete(this)?;
            let fields = table.delete(fields)?;
            (sender, fields)
        };
        _ = sender.0.send(fields);
        Ok(())
    }

    fn drop(&mut self, this: Resource<FieldsSender>) -> wasmtime::Result<()> {
        self.shared_table().delete(this)?;
        Ok(())
    }
}

#[async_trait]
impl<T: WasiHttpView> wasi::http::types::HostIsyswasfaReceiverOwnTrailers for T
where
    <T as IsyswasfaView>::State: WasiHttpState,
{
    async fn receive(
        state: <T as IsyswasfaView>::State,
        this: Resource<FieldsReceiver>,
    ) -> wasmtime::Result<Option<Resource<Fields>>> {
        let receiver = state.shared_table().delete(this)?;
        Ok(if let Ok(fields) = receiver.0.await {
            Some(state.shared_table().push(fields)?)
        } else {
            None
        })
    }

    fn drop(&mut self, this: Resource<FieldsReceiver>) -> wasmtime::Result<()> {
        self.shared_table().delete(this)?;
        Ok(())
    }
}

impl<T: WasiHttpView> wasi::http::types::HostFields for T {
    fn new(&mut self) -> wasmtime::Result<Resource<Fields>> {
        Ok(self.shared_table().push(Fields(Vec::new()))?)
    }

    fn from_list(
        &mut self,
        list: Vec<(String, Vec<u8>)>,
    ) -> wasmtime::Result<Result<Resource<Fields>, HeaderError>> {
        Ok(Ok(self.shared_table().push(Fields(list))?))
    }

    fn get(&mut self, this: Resource<Fields>, key: String) -> wasmtime::Result<Vec<Vec<u8>>> {
        Ok(self
            .shared_table()
            .get(&this)?
            .0
            .iter()
            .filter(|(k, _)| *k == key)
            .map(|(_, v)| v.clone())
            .collect())
    }

    fn has(&mut self, this: Resource<Fields>, key: String) -> wasmtime::Result<bool> {
        Ok(self
            .shared_table()
            .get(&this)?
            .0
            .iter()
            .any(|(k, _)| *k == key))
    }

    fn set(
        &mut self,
        this: Resource<Fields>,
        key: String,
        values: Vec<Vec<u8>>,
    ) -> wasmtime::Result<Result<(), HeaderError>> {
        let mut table = self.shared_table();
        let fields = table.get_mut(&this)?;
        fields.0.retain(|(k, _)| *k != key);
        fields
            .0
            .extend(values.into_iter().map(|v| (key.clone(), v)));
        Ok(Ok(()))
    }

    fn delete(
        &mut self,
        this: Resource<Fields>,
        key: String,
    ) -> wasmtime::Result<Result<(), HeaderError>> {
        self.shared_table()
            .get_mut(&this)?
            .0
            .retain(|(k, _)| *k != key);
        Ok(Ok(()))
    }

    fn append(
        &mut self,
        this: Resource<Fields>,
        key: String,
        value: Vec<u8>,
    ) -> wasmtime::Result<Result<(), HeaderError>> {
        self.shared_table().get_mut(&this)?.0.push((key, value));
        Ok(Ok(()))
    }

    fn entries(&mut self, this: Resource<Fields>) -> wasmtime::Result<Vec<(String, Vec<u8>)>> {
        Ok(self.shared_table().get(&this)?.0.clone())
    }

    fn clone(&mut self, this: Resource<Fields>) -> wasmtime::Result<Resource<Fields>> {
        let entries = self.shared_table().get(&this)?.0.clone();
        Ok(self.shared_table().push(Fields(entries))?)
    }

    fn drop(&mut self, this: Resource<Fields>) -> wasmtime::Result<()> {
        self.shared_table().delete(this)?;
        Ok(())
    }
}

#[async_trait]
impl<T: WasiHttpView> wasi::http::types::HostRequest for T
where
    <T as IsyswasfaView>::State: WasiHttpState,
{
    fn new(
        &mut self,
        headers: Resource<Fields>,
        body: Resource<InputStream>,
        trailers: Option<Resource<FieldsReceiver>>,
        options: Option<Resource<RequestOptions>>,
    ) -> wasmtime::Result<Resource<Request>> {
        let headers = self.shared_table().delete(headers)?;
        let body = self.table().delete(body)?;
        let trailers = if let Some(trailers) = trailers {
            Some(self.shared_table().delete(trailers)?)
        } else {
            None
        };
        let options = if let Some(options) = options {
            Some(self.table().delete(options)?)
        } else {
            None
        };

        Ok(self.shared_table().push(Request {
            method: Method::Get,
            scheme: None,
            path_with_query: None,
            authority: None,
            headers,
            body: Some(body),
            trailers,
            options,
        })?)
    }

    fn method(&mut self, this: Resource<Request>) -> wasmtime::Result<Method> {
        Ok(self.shared_table().get(&this)?.method.clone())
    }

    fn set_method(
        &mut self,
        this: Resource<Request>,
        method: Method,
    ) -> wasmtime::Result<Result<(), ()>> {
        self.shared_table().get_mut(&this)?.method = method;
        Ok(Ok(()))
    }

    fn scheme(&mut self, this: Resource<Request>) -> wasmtime::Result<Option<Scheme>> {
        Ok(self.shared_table().get(&this)?.scheme.clone())
    }

    fn set_scheme(
        &mut self,
        this: Resource<Request>,
        scheme: Option<Scheme>,
    ) -> wasmtime::Result<Result<(), ()>> {
        self.shared_table().get_mut(&this)?.scheme = scheme;
        Ok(Ok(()))
    }

    fn path_with_query(&mut self, this: Resource<Request>) -> wasmtime::Result<Option<String>> {
        Ok(self.shared_table().get(&this)?.path_with_query.clone())
    }

    fn set_path_with_query(
        &mut self,
        this: Resource<Request>,
        path_with_query: Option<String>,
    ) -> wasmtime::Result<Result<(), ()>> {
        self.shared_table().get_mut(&this)?.path_with_query = path_with_query;
        Ok(Ok(()))
    }

    fn authority(&mut self, this: Resource<Request>) -> wasmtime::Result<Option<String>> {
        Ok(self.shared_table().get(&this)?.authority.clone())
    }

    fn set_authority(
        &mut self,
        this: Resource<Request>,
        authority: Option<String>,
    ) -> wasmtime::Result<Result<(), ()>> {
        self.shared_table().get_mut(&this)?.authority = authority;
        Ok(Ok(()))
    }

    fn options(
        &mut self,
        this: Resource<Request>,
    ) -> wasmtime::Result<Option<Resource<RequestOptions>>> {
        // TODO: This should return an immutable child handle
        let options = self.shared_table().get(&this)?.options;
        Ok(if let Some(options) = options {
            Some(self.table().push(options)?)
        } else {
            None
        })
    }

    fn headers(&mut self, this: Resource<Request>) -> wasmtime::Result<Resource<Fields>> {
        // TODO: This should return an immutable child handle
        let headers = self.shared_table().get(&this)?.headers.clone();
        Ok(self.shared_table().push(headers)?)
    }

    fn body(
        &mut self,
        this: Resource<Request>,
    ) -> wasmtime::Result<Result<Resource<InputStream>, ()>> {
        // TODO: This should return a child handle
        let body = self
            .shared_table()
            .get_mut(&this)?
            .body
            .take()
            .ok_or_else(|| {
                anyhow!("todo: allow wasi:http/types#request.body to be called multiple times")
            })?;

        Ok(Ok(self.table().push(body)?))
    }

    async fn finish(
        state: <T as IsyswasfaView>::State,
        this: Resource<Request>,
    ) -> wasmtime::Result<Result<Option<Resource<Fields>>, ErrorCode>> {
        let trailers = state.shared_table().delete(this)?.trailers;
        Ok(Ok(if let Some(trailers) = trailers {
            if let Ok(trailers) = trailers.0.await {
                Some(state.shared_table().push(trailers)?)
            } else {
                None
            }
        } else {
            None
        }))
    }

    fn drop(&mut self, this: Resource<Request>) -> wasmtime::Result<()> {
        self.shared_table().delete(this)?;
        Ok(())
    }
}

#[async_trait]
impl<T: WasiHttpView> wasi::http::types::HostResponse for T
where
    <T as IsyswasfaView>::State: WasiHttpState,
{
    fn new(
        &mut self,
        headers: Resource<Fields>,
        body: Resource<InputStream>,
        trailers: Option<Resource<FieldsReceiver>>,
    ) -> wasmtime::Result<Resource<Response>> {
        let headers = self.shared_table().delete(headers)?;
        let body = self.table().delete(body)?;
        let trailers = if let Some(trailers) = trailers {
            Some(self.shared_table().delete(trailers)?)
        } else {
            None
        };

        Ok(self.shared_table().push(Response {
            status_code: 200,
            headers,
            body: Some(body),
            trailers,
        })?)
    }

    fn status_code(&mut self, this: Resource<Response>) -> wasmtime::Result<u16> {
        Ok(self.shared_table().get(&this)?.status_code)
    }

    fn set_status_code(
        &mut self,
        this: Resource<Response>,
        status_code: u16,
    ) -> wasmtime::Result<Result<(), ()>> {
        self.shared_table().get_mut(&this)?.status_code = status_code;
        Ok(Ok(()))
    }

    fn headers(&mut self, this: Resource<Response>) -> wasmtime::Result<Resource<Fields>> {
        // TODO: This should return an immutable child handle
        let headers = self.shared_table().get(&this)?.headers.clone();
        Ok(self.shared_table().push(headers)?)
    }

    fn body(
        &mut self,
        this: Resource<Response>,
    ) -> wasmtime::Result<Result<Resource<InputStream>, ()>> {
        // TODO: This should return a child handle
        let body = self
            .shared_table()
            .get_mut(&this)?
            .body
            .take()
            .ok_or_else(|| {
                anyhow!("todo: allow wasi:http/types#response.body to be called multiple times")
            })?;

        Ok(Ok(self.table().push(body)?))
    }

    async fn finish(
        state: <T as IsyswasfaView>::State,
        this: Resource<Response>,
    ) -> wasmtime::Result<Result<Option<Resource<Fields>>, ErrorCode>> {
        let trailers = state.shared_table().delete(this)?.trailers;
        Ok(Ok(if let Some(trailers) = trailers {
            if let Ok(trailers) = trailers.0.await {
                Some(state.shared_table().push(trailers)?)
            } else {
                None
            }
        } else {
            None
        }))
    }

    fn drop(&mut self, this: Resource<Response>) -> wasmtime::Result<()> {
        self.shared_table().delete(this)?;
        Ok(())
    }
}

impl<T: WasiHttpView> wasi::http::types::HostRequestOptions for T {
    fn new(&mut self) -> wasmtime::Result<Resource<RequestOptions>> {
        Ok(self.shared_table().push(RequestOptions::default())?)
    }

    fn connect_timeout(&mut self, this: Resource<RequestOptions>) -> wasmtime::Result<Option<u64>> {
        Ok(self.table().get(&this)?.connect_timeout)
    }

    fn set_connect_timeout(
        &mut self,
        this: Resource<RequestOptions>,
        connect_timeout: Option<u64>,
    ) -> wasmtime::Result<Result<(), RequestOptionsError>> {
        self.table().get_mut(&this)?.connect_timeout = connect_timeout;
        Ok(Ok(()))
    }

    fn first_byte_timeout(
        &mut self,
        this: Resource<RequestOptions>,
    ) -> wasmtime::Result<Option<u64>> {
        Ok(self.table().get(&this)?.first_byte_timeout)
    }

    fn set_first_byte_timeout(
        &mut self,
        this: Resource<RequestOptions>,
        first_byte_timeout: Option<u64>,
    ) -> wasmtime::Result<Result<(), RequestOptionsError>> {
        self.table().get_mut(&this)?.first_byte_timeout = first_byte_timeout;
        Ok(Ok(()))
    }

    fn between_bytes_timeout(
        &mut self,
        this: Resource<RequestOptions>,
    ) -> wasmtime::Result<Option<u64>> {
        Ok(self.table().get(&this)?.between_bytes_timeout)
    }

    fn set_between_bytes_timeout(
        &mut self,
        this: Resource<RequestOptions>,
        between_bytes_timeout: Option<u64>,
    ) -> wasmtime::Result<Result<(), RequestOptionsError>> {
        self.table().get_mut(&this)?.between_bytes_timeout = between_bytes_timeout;
        Ok(Ok(()))
    }

    fn drop(&mut self, this: Resource<RequestOptions>) -> wasmtime::Result<()> {
        self.table().delete(this)?;
        Ok(())
    }
}

impl<T: WasiHttpView> wasi::http::types::Host for T
where
    <T as IsyswasfaView>::State: WasiHttpState,
{
    fn isyswasfa_pipe_own_trailers(
        &mut self,
    ) -> wasmtime::Result<(Resource<FieldsSender>, Resource<FieldsReceiver>)> {
        let (tx, rx) = oneshot::channel();
        let mut table = self.shared_table();
        let tx = table.push(FieldsSender(tx))?;
        let rx = table.push(FieldsReceiver(rx))?;
        Ok((tx, rx))
    }

    fn http_error_code(&mut self, _error: Resource<Error>) -> wasmtime::Result<Option<ErrorCode>> {
        Err(anyhow!("todo: implement wasi:http/types#http-error-code"))
    }
}

pub fn add_to_linker<T: WasiHttpView>(linker: &mut Linker<T>) -> wasmtime::Result<()>
where
    <T as IsyswasfaView>::State: WasiHttpState,
{
    wasi::http::types::add_to_linker(linker, |ctx| ctx)
}