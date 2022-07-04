// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.constructs.Construct;
import software.amazon.awscdk.Stack;
import software.amazon.awscdk.StackProps;

import software.amazon.awscdk.services.ec2.Vpc;
import software.amazon.awscdk.services.ec2.GatewayVpcEndpoint;
import software.amazon.awscdk.services.ec2.GatewayVpcEndpointOptions;
import software.amazon.awscdk.services.ec2.GatewayVpcEndpointAwsService;
import software.amazon.awscdk.services.ec2.SecurityGroup;
import software.amazon.awscdk.services.ec2.Peer;
import software.amazon.awscdk.services.ec2.Port;

public class ClientServerStack extends Stack {
    private final Vpc vpc;

    public ClientServerStack(final Construct parent, final String id, final ClientServerStackProps props) {
        super(parent, id, props);

        String stackType  = props.getStackType();
        String cidr = props.getCidr();
        
        //All construct names are for descriptive purposes only
        this.vpc = Vpc.Builder.create(this, stackType + "-vpc")
            .maxAzs(1)
            .enableDnsSupport(true)
            .enableDnsHostnames(true)
            .cidr(cidr)
            .build();

        SecurityGroup.fromSecurityGroupId(this, "vpc-sec-group", this.vpc.getVpcDefaultSecurityGroup())
            .addIngressRule(Peer.anyIpv4(), Port.allTraffic());

        GatewayVpcEndpoint s3Endpoint = this.vpc.addGatewayEndpoint("s3-endpoint",
            GatewayVpcEndpointOptions.builder()
            .service(GatewayVpcEndpointAwsService.S3)
            .build());
    }

    public Vpc getVpc() {
        return this.vpc;
    }

}                                         
