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
import software.amazon.awscdk.services.ec2.BastionHostLinux;
import software.amazon.awscdk.services.ec2.*;
import software.amazon.awscdk.services.ssm.StringParameter;


import software.amazon.awscdk.services.apigateway.LambdaRestApi;
import software.amazon.awscdk.services.lambda.Code;
import software.amazon.awscdk.services.lambda.Function;
import software.amazon.awscdk.services.lambda.Runtime;
import software.amazon.awscdk.RemovalPolicy;

import java.util.HashMap;


public class ClientServerStack extends Stack {
    private String cidr;
    private Vpc vpc;

    public ClientServerStack(final Construct parent, final String id) {
        this(parent, id, null);
    }

    public ClientServerStack(final Construct parent, final String id, final ClientServerStackProps props) {
        super(parent, id, props);

        String instanceType = props.getInstanceType();
        String stackType  = props.getStackType();
        this.cidr = props.getCidr();

        this.vpc = Vpc.Builder.create(this, stackType + "-vpc")
            .maxAzs(1)
            .cidr(cidr)
            .build();

        SecurityGroup.fromSecurityGroupId(this, "vpc-sec-group", this.vpc.getVpcDefaultSecurityGroup())
            .addIngressRule(Peer.anyIpv4(), Port.icmpPing(), "Allow ping anywhere.");

        GatewayVpcEndpoint s3Endpoint = this.vpc.addGatewayEndpoint("s3-endpoint",
            GatewayVpcEndpointOptions.builder()
            .service(GatewayVpcEndpointAwsService.S3)
            .build());

        StringParameter.Builder.create(this, stackType + "-vpc-id")
            .parameterName(stackType + "-vpc-id")
            .stringValue(this.vpc.getVpcId())
            .build();

        StringParameter.Builder.create(this, stackType + "-cidr")
            .parameterName(stackType + "-cidr")
            .stringValue(this.vpc.getVpcCidrBlock())
            .build();

        /*
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

        Ec2TaskDefinition task = Ec2TaskDefinition.Builder
            .create(this, stackType + "-task")
            .build();
        task.addContainer(); 
        
        Ec2Service.Builder.create(this, "ec2service-" + stackType)
            .cluster(cluster)
            .taskDefinition(task)
            .build();
        */

        BastionHostLinux testInstance = BastionHostLinux.Builder.create(this, "testInstance")
            .vpc(vpc)
            .securityGroup(SecurityGroup.fromSecurityGroupId(this, stackType + "vpc-sec-group", this.vpc.getVpcDefaultSecurityGroup()))
            .build();

        Bucket metricsBucket = props.getBucket();
        final HashMap<String, String> environment = new HashMap<>();
        environment.put("BUCKET_NAME", metricsBucket.getBucketName());
        final Function test = Function.Builder.create(this, "testLambda")
            .vpc(vpc)
            .runtime(Runtime.NODEJS_14_X)    // execution environment
            .code(Code.fromAsset("lambda"))  // code loaded from the "lambda" directory
            .handler("bucket.handler")        // file is "hello", function is "handler"
            .environment(environment)
            .build();

        metricsBucket.grantReadWrite(test);        

        LambdaRestApi.Builder.create(this, "Endpoint")
            .handler(test)
            .build();

    }

    public Vpc getVpc() {
        return this.vpc;
    }

    public String getCidr() {
        return this.cidr;
    }

}                                         
