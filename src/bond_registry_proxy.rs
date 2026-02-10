use multiversx_sc::proxy_imports::*;

pub struct BondRegistryProxy;

impl<Env, From, To, Gas> TxProxyTrait<Env, From, To, Gas> for BondRegistryProxy
where
    Env: TxEnv,
    From: TxFrom<Env>,
    To: TxTo<Env>,
    Gas: TxGas<Env>,
{
    type TxProxyMethods = BondRegistryProxyMethods<Env, From, To, Gas>;

    fn proxy_methods(self, tx: Tx<Env, From, To, (), Gas, (), ()>) -> Self::TxProxyMethods {
        BondRegistryProxyMethods { wrapped_tx: tx }
    }
}

pub struct BondRegistryProxyMethods<Env, From, To, Gas>
where
    Env: TxEnv,
    From: TxFrom<Env>,
    To: TxTo<Env>,
    Gas: TxGas<Env>,
{
    wrapped_tx: Tx<Env, From, To, (), Gas, (), ()>,
}

impl<Env, From, To, Gas> BondRegistryProxyMethods<Env, From, To, Gas>
where
    Env: TxEnv,
    Env::Api: VMApi,
    From: TxFrom<Env>,
    To: TxTo<Env>,
    Gas: TxGas<Env>,
{
    pub fn get_agent_name<Arg0: ProxyArg<ManagedAddress<Env::Api>>>(
        self,
        agent: Arg0,
    ) -> TxTypedCall<Env, From, To, NotPayable, Gas, ManagedBuffer<Env::Api>> {
        self.wrapped_tx
            .payment(NotPayable)
            .raw_call("getAgentName")
            .argument(&agent)
            .original_result()
    }
}
