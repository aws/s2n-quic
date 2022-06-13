// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.constructs.Construct;
import software.amazon.awscdk.Stack;
import software.amazon.awscdk.StackProps;
import software.amazon.awscdk.services.ec2.CfnVPCPeeringConnection;
import software.amazon.awscdk.services.ec2.Vpc;
import software.amazon.awscdk.services.ec2.*;

class PeeringConnectionStack extends Stack {
    public PeeringConnectionStack(final Construct parent, final String id) {
        this(parent, id, null);
    }

    public PeeringConnectionStack(final Construct parent, final String id, final PeeringStackProps props) {
        super(parent, id, props);

        //Vpc peering connection between client-server vpc's
        CfnVPCPeeringConnection conn = CfnVPCPeeringConnection.Builder
            .create(this, "vpc-peering-connection")
            .vpcId(props.getVpcServer().getVpcId())
            .peerVpcId(props.getVpcClient().getVpcId())
            .build();

        int counter = 1;
        //Establishing client-to-server connections between private subnets
        for (ISubnet subnet: props.getVpcClient().getPrivateSubnets()) {
            CfnRoute.Builder.create(this, 
            "client-to-server" + Integer.toString(counter))
            .destinationCidrBlock(props.getVpcServer().getVpcCidrBlock())
            .routeTableId(subnet.getRouteTable().getRouteTableId())
            .vpcPeeringConnectionId(conn.getRef())
            .build();
            counter++;
        }  

        counter = 1;
        //Establishing server-to-client connections between private subnets
        for (ISubnet subnet: props.getVpcServer().getPrivateSubnets()) {
            CfnRoute.Builder.create(this, 
            "server-to-client" + Integer.toString(counter))
            .destinationCidrBlock(props.getVpcClient().getVpcCidrBlock())
            .routeTableId(subnet.getRouteTable().getRouteTableId())
            .vpcPeeringConnectionId(conn.getRef())
            .build();
            counter++;
        }        
    }
}