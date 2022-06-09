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


public class ClientStack extends Stack {
    public ClientStack(final Construct parent, final String id) {
        this(parent, id, null, null, null);
    }

    public ClientStack(final Construct parent, final String id, Bucket metricsBucket,
        String instanceType, final StackProps props) {
        super(parent, id, props);

        //String bucketName = metricsBucket.getBucketArn();
        Vpc vpc = Vpc.Builder.create(this, "client-vpc")
            .build();

        GatewayVpcEndpoint s3Endpoint = vpc.addGatewayEndpoint("s3-endpoint",
            GatewayVpcEndpointOptions.builder()
            .service(GatewayVpcEndpointAwsService.S3)
            .build());
            
        Cluster cluster = Cluster.Builder.create(this, "client-cluster")
            .vpc(vpc)
            .build();
        
        AutoScalingGroup asg = AutoScalingGroup.Builder.create(this, "asg")
            .vpc(vpc)
            .instanceType(new InstanceType(instanceType))
            .machineImage(EcsOptimizedImage.amazonLinux2())
            .build();

        AsgCapacityProvider asgProvider = AsgCapacityProvider.Builder.create(this, "asg-provider")
            .autoScalingGroup(asg)
            .build();
        
        cluster.addAsgCapacityProvider(asgProvider);

        /*
        Ec2TaskDefinition clientTask = Ec2TaskDefinition.Builder
            .create(this, "client-task")
            .build();
        clientTask.addContainer(); 
        
        Ec2Service.Builder.create(this, "ec2service-client")
            .cluster(cluster)
            .taskDefinition(clientTask)
            .build(); */
    }
}                                         
