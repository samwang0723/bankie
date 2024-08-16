#[allow(dead_code)]
#[derive(Debug)]
pub struct Tenant {
    pub id: i32,
    pub name: String,
    pub jwt: String,
    pub status: String,
    pub scope: Option<String>,
}