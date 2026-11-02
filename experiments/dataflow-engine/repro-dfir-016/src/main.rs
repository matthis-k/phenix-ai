fn main() {
    let mut graph = dfir_rs::dfir_syntax! {
        input = source_iter([1_u64,2]) -> partition(|v: &u64, _n| (*v % 2) as usize);
        input[0] -> a; input[1] -> a;
        a = union() -> for_each(|_| {});
    };
    graph.run_available_sync();
}
