#![allow(dead_code)]

use phenix_sdk as phenix;

#[derive(phenix::PhenixValue, phenix::PhenixContract)]
#[phenix(id = "fixture.external.request@1")]
struct Request {
    value: String,
}

#[phenix::interface("fixture.external.echo@1")]
struct Echo;

#[phenix::component]
struct EchoComponent;

#[phenix::component]
impl EchoComponent {
    #[phenix(export("fixture.external.echo.run@1"))]
    fn run(&self, request: Request) -> Request {
        request
    }
}

#[phenix::plugin("fixture.external.plugin")]
struct Plugin {
    #[phenix(component)]
    echo: EchoComponent,
}
