// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.constructs.Construct;
import software.amazon.awscdk.Stack;
import software.amazon.awscdk.StackProps;
import software.amazon.awscdk.services.ec2.CfnVPCPeeringConnection;
import software.amazon.awscdk.services.ec2.Vpc;
import software.amazon.awscdk.services.ec2.*;
import software.amazon.awscdk.services.ssm.StringParameter;
import software.amazon.awscdk.services.ssm.IStringParameter;

class PeeringConnectionStack extends Stack {
    private String ref;

    public PeeringConnectionStack(final Construct parent, final String id) {
        this(parent, id, null);
    }

    public PeeringConnectionStack(final Construct parent, final String id, final PeeringStackProps props) {
        super(parent, id, props);

        String stackType = props.getStackType();
        int counter = 1;

        if (stackType.equals("server")) {
            String serverVpcId = StringParameter.fromStringParameterName(this, "server-vpc-id",
                "server-vpc-id").getStringValue();

            String clientVpcId = new SSMParameterReader(this, "client-vpc-id-reader", SSMParameterReaderProps.builder()
                .sdkCall("client-vpc-id", props.getRegion())
                .policy()
                .build())
                .getParameterValue();

            
            String cidr = new SSMParameterReader(this, "client-cidr-reader", SSMParameterReaderProps.builder()
                .sdkCall("client-cidr", props.getRegion())
                .policy()
                .build())
                .getParameterValue();
            
            /*
            String clientVpcId = StringParameter.fromStringParameterName(this, "client-vpc-id",
                "client-vpc-id").getStringValue();
            
            
            String cidr = StringParameter.fromStringParameterName(this, "client-cidr",
                "client-cidr").getStringValue();
            */

            //Vpc peering connection between client-server vpc's
            CfnVPCPeeringConnection conn = CfnVPCPeeringConnection.Builder
                .create(this, "vpc-peering-connection")
                .vpcId(serverVpcId)
                .peerVpcId(clientVpcId)
                .peerRegion(props.getRegion())
                .build();

            //Establishing server-to-client connections between private subnets
            for (ISubnet subnet: props.getVpcServer().getPrivateSubnets()) {
                CfnRoute.Builder.create(this, 
                "server-to-client" + Integer.toString(counter))
                .destinationCidrBlock(cidr)
                .routeTableId(subnet.getRouteTable().getRouteTableId())
                .vpcPeeringConnectionId(conn.getRef())
                .build();
                counter++;
            }

            StringParameter.Builder.create(this, "conn-ref")
                .parameterName("conn-ref")
                .stringValue(conn.getRef())
                .build();
        } else {
            String cidr = new SSMParameterReader(this, "server-cidr-reader", SSMParameterReaderProps.builder()
                .sdkCall("server-cidr", props.getRegion())
                .policy()
                .build())
                .getParameterValue();

            String connRef = new SSMParameterReader(this, "conn-ref-reader", SSMParameterReaderProps.builder()
                .sdkCall("conn-ref", props.getRegion())
                .policy()
                .build())
                .getParameterValue();

            /*
            String connRef = StringParameter.fromStringParameterName(this, "conn-ref",
                "conn-ref").getStringValue();
            */

            //Establishing client-to-server connections between private subnets
            for (ISubnet subnet: props.getVpcClient().getPrivateSubnets()) {
                CfnRoute.Builder.create(this, 
                "client-to-server" + Integer.toString(counter))
                .destinationCidrBlock(cidr)
                .routeTableId(subnet.getRouteTable().getRouteTableId())
                .vpcPeeringConnectionId(connRef)
                .build();
                counter++;
            }  
        }          
    }

    public String getRef() {
        return this.ref;
    }
}