#![allow(dead_code)]

#[derive(phenix::PhenixValue, phenix::PhenixContract)]
#[phenix(id = "fixture.renamed.request@1")]
struct Request {
    value: String,
}

#[phenix::interface("fixture.renamed.echo@1")]
struct Echo;

#[phenix::component]
struct EchoComponent;

#[phenix::component]
impl EchoComponent {
    #[phenix(export("fixture.renamed.echo.run@1"))]
    fn run(&self, request: Request) -> Request {
        request
    }
}

#[phenix::plugin("fixture.renamed.plugin")]
struct Plugin {
    #[phenix(component)]
    echo: EchoComponent,
}
