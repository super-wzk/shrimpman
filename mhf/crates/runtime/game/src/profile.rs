#[derive(Clone, Copy)]
pub struct MhfLaunchProfile<'a> {
    pub game_dll: &'a str,
    pub ini_name: &'a str,
    pub instance_mutex_prefix: &'a str,
    pub ready_mutex_prefix: &'a str,
    pub host_message: &'a str,
}
