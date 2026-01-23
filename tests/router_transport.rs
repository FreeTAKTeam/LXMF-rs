use lxmf::reticulum::Adapter;
use lxmf::router::Router;

#[test]
fn router_accepts_reticulum_adapter() {
    let adapter = Adapter::new();
    let _router = Router::with_adapter(adapter);
}
