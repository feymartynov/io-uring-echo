pub type Id = u32;

#[derive(Debug)]
#[repr(u32)]
pub enum Route {
    Accept,
    ClientRead(Id),
    ClientWrite(Id),
}

impl From<Route> for u64 {
    fn from(route: Route) -> Self {
        unsafe { std::mem::transmute(route) }
    }
}

impl From<u64> for Route {
    fn from(value: u64) -> Self {
        unsafe { std::mem::transmute(value) }
    }
}
