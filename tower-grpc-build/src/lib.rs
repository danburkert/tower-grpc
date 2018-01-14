extern crate codegen;
extern crate prost_build;

mod client;
mod server;
mod names;

use std::io;
use std::cell::RefCell;
use std::fmt::Write;
use std::path::Path;
use std::rc::Rc;
use std::ascii::AsciiExt;

/// Code generation configuration
pub struct Config {
    prost: prost_build::Config,
    inner: Rc<RefCell<Inner>>,
}

struct Inner {
    build_client: bool,
    build_server: bool,
}

struct ServiceGenerator {
    client: client::ServiceGenerator,
    server: server::ServiceGenerator,
    inner: Rc<RefCell<Inner>>,
    root_scope: RefCell<codegen::Scope>,
}

impl Config {
    /// Returns a new `Config` with pre-configured prost.
    ///
    /// You can tweak the configuration how the proto buffers are generated and use this config.
    pub fn from_prost(mut prost: prost_build::Config) -> Self {
        let inner = Rc::new(RefCell::new(Inner {
            // Enable client code gen by default
            build_client: true,

            // Disable server code gen by default
            build_server: false,
        }));

        let root_scope = RefCell::new(codegen::Scope::new());

        // Set the service generator
        prost.service_generator(Box::new(ServiceGenerator {
            client: client::ServiceGenerator,
            server: server::ServiceGenerator,
            inner: inner.clone(),
            root_scope,
        }));

        Config {
            prost,
            inner,
        }
    }

    /// Returns a new `Config` with default values.
    pub fn new() -> Self {
        Self::from_prost(prost_build::Config::new())
    }

    /// Enable gRPC client code generation
    pub fn enable_client(&mut self, enable: bool) -> &mut Self {
        self.inner.borrow_mut().build_client = enable;
        self
    }

    /// Enable gRPC server code generation
    pub fn enable_server(&mut self, enable: bool) -> &mut Self {
        self.inner.borrow_mut().build_server = enable;
        self
    }

    /// Generate code
    pub fn build<P>(&self, protos: &[P], includes: &[P]) -> io::Result<()>
    where P: AsRef<Path>,
    {
        self.prost.compile_protos(protos, includes)
    }
}

impl prost_build::ServiceGenerator for ServiceGenerator {

    fn generate(&self, service: prost_build::Service, _buf: &mut String) {
        // Note that neither this implementation of `generate` nor the
        // implementations for `client::ServiceGenerator` and
        // `server::ServiceGenerator` will actually output any code to the
        // buffer; all code is written out in the implementation of the
        // `ServiceGenerator::finalize` function on this type.
        let inner = self.inner.borrow();
        let mut root = self.root_scope.borrow_mut();

        if inner.build_client {
            self.client.generate(&service, &mut root);
        }

        if inner.build_server {
            self.server.generate(&service, &mut root);
        }
    }

    fn finalize(&self, buf: &mut String) {
        // Rather than outputting each service to the buffer as it's generated,
        // we generate the code in our root `codegen::Scope`, which is shared
        // between the generation of each service in the proto file. Unlike a
        // string, codegen provides us with something not unlike a simplified
        // Rust AST, making it easier for us to add new items to modules
        // defined by previous service generator invocations. As we want to
        // output the client and server implementations for each service in the
        // proto file in one `client` or `server` module in the generated code,
        // we wait until all the services have been generated before actually
        // outputting to the buffer.
        let mut fmt = codegen::Formatter::new(buf);
        self.root_scope.borrow()
            .fmt(&mut fmt)
            .expect("formatting root scope failed!");

        // reset the root scope so that the service generator is ready to
        // generate another file. this prevents the code generated for *this*
        // file being present in the next file.
        *self.root_scope.borrow_mut() = codegen::Scope::new();

    }
}

// ===== utility fns =====

fn method_path(service: &prost_build::Service, method: &prost_build::Method) -> String {
    format!("\"/{}.{}/{}\"",
            service.package,
            service.proto_name,
            method.proto_name)
}

fn lower_name(name: &str) -> String {
    let mut ret = String::new();

    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() {
            if i != 0 {
                ret.push('_');
            }

            ret.push(ch.to_ascii_lowercase());
        } else {
            ret.push(ch);
        }
    }

    ret
}

fn super_import(ty: &str, level: usize, scope: &mut codegen::Scope) -> String {
    let mut v: Vec<&str> = ty.split("::").collect();
    for _ in 0..level {
        v.insert(0, "super");
    }

    // index of the first path element in `ty` that concretely names an item
    // (i.e., isn't super). a `use` statement may only end with a concrete name;
    // you can't `use super::super::super;`.
    let first_concrete_name = v.iter()
        .position(|s| s != &"super")
        .expect("got a type name that was just a string of \"::super\"s!");

    if first_concrete_name == v.len() - 1 {
        // the first concrete name in the path is the actual type name.
        // in this case, we can't come up with a reasonable `use` statement for
        // it, since if we import the name directly, it may clash with names
        // defined in this namespace, but we can't import its containing
        // namespace, because the containing namespace is `super`, and `use`
        // statements have to end in a concrete name.
        // println!("cargo:warning={:?} does not need super", v);
        v.join("::")
    } else {

        // println!("cargo:warning={:?} needs super; fcn={}", v, first_concrete_name);
        let last = v[v.len()-2..].join("::");
        let path = v[..v.len()-2].join("::");
        scope.import(&path, &last);
        last
    }

}

fn unqualified(ty: &str) -> &str {
    ty.rsplit("::").next().unwrap_or(ty)
}
