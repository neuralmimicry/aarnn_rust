// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
pub mod bridge_route_table;
pub mod endpoint_table;
pub mod host_subscription_table;
pub mod router;
pub mod synapse_table;

pub use bridge_route_table::{BridgeRoute, BridgeRouteTable, SynapseRange};
pub use endpoint_table::{
    EndpointId, EndpointLocation, EndpointRoute, EndpointType, LineSynapseIndex,
};
pub use host_subscription_table::HostSubscriptionTable;
pub use router::{RouteAction, Router};
pub use synapse_table::{LocalSynapseTable, SynapseEndpoint, SynapseEntry};
