
pub fn running_on_fly() -> bool {
    current_app_name().is_some()
}

pub fn current_app_name() -> Option<String> {
    std::env::var("FLY_APP_NAME").ok()
}