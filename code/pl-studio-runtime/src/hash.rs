pub(crate) fn canonical_json_hash(value: &serde_json::Value) -> String {
    let mut value = value.clone();
    value.sort_all_objects();
    pl_core::context::content_hash(value.to_string().as_bytes())
}

pub(crate) fn merge_costs(
    target: &mut Vec<pl_protocol::RuntimeCostAmount>,
    incoming: &[pl_protocol::RuntimeCostAmount],
) {
    for cost in incoming {
        if let Some(current) = target
            .iter_mut()
            .find(|current| current.currency == cost.currency)
        {
            current.amount += cost.amount;
        } else {
            target.push(cost.clone());
        }
    }
    target.sort_by(|left, right| left.currency.cmp(&right.currency));
}
