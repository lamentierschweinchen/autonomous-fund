use multiversx_sc::proxy_imports::*;

pub struct UptimeProxy;

impl<Env, From, To, Gas> TxProxyTrait<Env, From, To, Gas> for UptimeProxy
where
    Env: TxEnv,
    From: TxFrom<Env>,
    To: TxTo<Env>,
    Gas: TxGas<Env>,
{
    type TxProxyMethods = UptimeProxyMethods<Env, From, To, Gas>;

    fn proxy_methods(self, tx: Tx<Env, From, To, (), Gas, (), ()>) -> Self::TxProxyMethods {
        UptimeProxyMethods { wrapped_tx: tx }
    }
}

pub struct UptimeProxyMethods<Env, From, To, Gas>
where
    Env: TxEnv,
    From: TxFrom<Env>,
    To: TxTo<Env>,
    Gas: TxGas<Env>,
{
    wrapped_tx: Tx<Env, From, To, (), Gas, (), ()>,
}

impl<Env, From, To, Gas> UptimeProxyMethods<Env, From, To, Gas>
where
    Env: TxEnv,
    Env::Api: VMApi,
    From: TxFrom<Env>,
    To: TxTo<Env>,
    Gas: TxGas<Env>,
{
    pub fn get_lifetime_info<Arg0: ProxyArg<ManagedAddress<Env::Api>>>(
        self,
        agent: Arg0,
    ) -> TxTypedCall<Env, From, To, NotPayable, Gas, MultiValue4<u64, u64, u64, u64>> {
        self.wrapped_tx
            .payment(NotPayable)
            .raw_call("getLifetimeInfo")
            .argument(&agent)
            .original_result()
    }
}
