// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.constructs.Construct;
import software.amazon.awscdk.Stack;
import software.amazon.awscdk.StackProps;
import software.amazon.awscdk.services.s3.Bucket;
import software.amazon.awscdk.services.ecs.Cluster;
import software.amazon.awscdk.services.ec2.Vpc;
import software.amazon.awscdk.services.ecs.*;
import software.amazon.awscdk.services.autoscaling.AutoScalingGroup;
import software.amazon.awscdk.services.ec2.InstanceType;
import software.amazon.awscdk.services.ec2.GatewayVpcEndpoint;
import software.amazon.awscdk.services.ec2.GatewayVpcEndpointOptions;
import software.amazon.awscdk.services.ec2.GatewayVpcEndpointAwsService;

import software.amazon.awscdk.services.apigateway.LambdaRestApi;
import software.amazon.awscdk.services.lambda.Code;
import software.amazon.awscdk.services.lambda.Function;
import software.amazon.awscdk.services.lambda.Runtime;
import software.amazon.awscdk.RemovalPolicy;

import java.util.HashMap;


public class ClientServerStack extends Stack {
    private Vpc vpc;
    public ClientServerStack(final Construct parent, final String id) {
        this(parent, id, null);
    }

    public ClientServerStack(final Construct parent, final String id, final ClientServerStackProps props) {
        super(parent, id, props);

        String instanceType = props.getInstanceType();
        String stackType  = props.getStackType();

        this.vpc = Vpc.Builder.create(this, stackType + "-vpc")
            .maxAzs(1)
            .cidr(props.getCidr())
            .build();

        GatewayVpcEndpoint s3Endpoint = vpc.addGatewayEndpoint("s3-endpoint",
            GatewayVpcEndpointOptions.builder()
            .service(GatewayVpcEndpointAwsService.S3)
            .build());

        Bucket metricsBucket = props.getBucket();

        Cluster cluster = Cluster.Builder.create(this, stackType + "-cluster")
            .vpc(vpc)
            .build();
        
        AutoScalingGroup asg = AutoScalingGroup.Builder.create(this, stackType + "-asg")
            .vpc(vpc)
            .instanceType(new InstanceType(instanceType))
            .machineImage(EcsOptimizedImage.amazonLinux2())
            .minCapacity(0)
            .build();

        AsgCapacityProvider asgProvider = AsgCapacityProvider.Builder.create(this, stackType + "-asg-provider")
            .autoScalingGroup(asg)
            .build();
        
        cluster.addAsgCapacityProvider(asgProvider);

        /* Docker image not yet generated
        Ec2TaskDefinition task = Ec2TaskDefinition.Builder
            .create(this, stackType + "-task")
            .build();
        task.addContainer(); 
        
        Ec2Service.Builder.create(this, "ec2service-" + stackType)
            .cluster(cluster)
            .taskDefinition(task)
            .build(); */

    }

    public Vpc getVpc() {
        return this.vpc;
    }

}                                         
