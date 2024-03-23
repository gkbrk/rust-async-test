pub trait FutureMap<FutureOut> {
    async fn map<Fn, Ou>(self, f: Fn) -> Ou
    where
        Self: std::future::Future<Output = FutureOut> + Sized,
        Fn: FnOnce(FutureOut) -> Ou;
}

impl<FutureOut, F> FutureMap<FutureOut> for F
where
    F: std::future::Future<Output = FutureOut>,
{
    async fn map<Fn, Ou>(self, f: Fn) -> Ou
    where
        Fn: FnOnce(FutureOut) -> Ou,
    {
        let x = self.await;
        f(x)
    }
}
