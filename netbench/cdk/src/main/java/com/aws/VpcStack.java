// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.amazon.awscdk.PhysicalName;
import software.amazon.awscdk.RemovalPolicy;
import software.amazon.awscdk.Stack;
import software.amazon.awscdk.services.ec2.GatewayVpcEndpoint;
import software.amazon.awscdk.services.ec2.GatewayVpcEndpointAwsService;
import software.amazon.awscdk.services.ec2.GatewayVpcEndpointOptions;
import software.amazon.awscdk.services.ec2.Peer;
import software.amazon.awscdk.services.ec2.Port;
import software.amazon.awscdk.services.ec2.SecurityGroup;
import software.amazon.awscdk.services.ec2.Vpc;
import software.amazon.awscdk.services.s3.Bucket;
import software.constructs.Construct;

public class VpcStack extends Stack {
    private final Vpc vpc;
    private Bucket metricsBucket;

    public VpcStack(final Construct parent, final String id, final VpcStackProps props) {
        super(parent, id, props);

        String cidr = props.getCidr();
        
        //All construct names are for descriptive purposes only
        this.vpc = Vpc.Builder.create(this, "client-server-vpc")
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

        metricsBucket = Bucket.Builder.create(this, "MetricsReportBucket")
            .bucketName(PhysicalName.GENERATE_IF_NEEDED)
            .removalPolicy(RemovalPolicy.DESTROY)
            .autoDeleteObjects(true)
            .build();
    }

    public Vpc getVpc() {
        return this.vpc;
    }

    public Bucket getBucket() {
        return metricsBucket;
    }

}                                         
