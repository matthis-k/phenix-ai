//! Stable Rust inference experiments used to choose the public handler adapter.

struct Context;
struct AddArgs {
    left: i64,
    right: i64,
}

fn closure_adapter<A, F>(handler: F) -> impl Fn(A, &Context) -> i64
where
    F: Fn(A, &Context) -> i64,
{
    handler
}

trait TypedHandler {
    type Args;
    type Output;

    fn call(&self, args: Self::Args, context: &Context) -> Self::Output;
}

struct AddHandler;

impl TypedHandler for AddHandler {
    type Args = AddArgs;
    type Output = i64;

    fn call(&self, args: Self::Args, _: &Context) -> Self::Output {
        args.left + args.right
    }
}

fn execute_handler<H: TypedHandler>(handler: &H, args: H::Args) -> H::Output {
    handler.call(args, &Context)
}

fn add_function(args: AddArgs, _: &Context) -> i64 {
    args.left + args.right
}

#[test]
fn closure_function_item_and_associated_type_handler_patterns_compile() {
    let closure = closure_adapter(|args: AddArgs, _: &Context| args.left + args.right);
    assert_eq!(closure(AddArgs { left: 2, right: 5 }, &Context), 7);

    let function = closure_adapter(add_function);
    assert_eq!(function(AddArgs { left: 3, right: 4 }, &Context), 7);

    assert_eq!(
        execute_handler(&AddHandler, AddArgs { left: 1, right: 8 }),
        9
    );
}
