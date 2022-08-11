// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.constructs.Construct;
import software.amazon.awscdk.RemovalPolicy;
import software.amazon.awscdk.Stack;
import software.amazon.awscdk.services.s3.Bucket;
import software.amazon.awscdk.services.autoscaling.AutoScalingGroup;
import software.amazon.awscdk.services.servicediscovery.DnsRecordType;
import software.amazon.awscdk.services.servicediscovery.PrivateDnsNamespace;
import software.amazon.awscdk.services.ecs.Cluster;
import software.amazon.awscdk.services.ecs.ContainerImage;
import software.amazon.awscdk.services.ecs.EcsOptimizedImage;
import software.amazon.awscdk.services.ecs.AsgCapacityProvider;
import software.amazon.awscdk.services.ecs.Ec2TaskDefinition;
import software.amazon.awscdk.services.ecs.ContainerDefinitionOptions;
import software.amazon.awscdk.services.ecs.Ec2Service;
import software.amazon.awscdk.services.ecs.CapacityProviderStrategy;
import software.amazon.awscdk.services.ecs.NetworkMode;
import software.amazon.awscdk.services.ecs.PortMapping;
import software.amazon.awscdk.services.ecs.AmiHardwareType;
import software.amazon.awscdk.services.ecs.CloudMapOptions;
import software.amazon.awscdk.services.ecs.AwsLogDriverProps;
import software.amazon.awscdk.services.ecs.LogDriver;
import software.amazon.awscdk.services.ecs.ContainerDefinition;

import software.amazon.awscdk.services.logs.LogGroup;
import software.amazon.awscdk.services.logs.RetentionDays;

import software.amazon.awscdk.services.lambda.Function;
import software.amazon.awscdk.services.lambda.Runtime;
import software.amazon.awscdk.services.lambda.Code;

import software.amazon.awscdk.services.stepfunctions.tasks.EcsRunTask;
import software.amazon.awscdk.services.stepfunctions.IntegrationPattern;
import software.amazon.awscdk.services.stepfunctions.tasks.EcsEc2LaunchTarget;
import software.amazon.awscdk.services.stepfunctions.tasks.ContainerOverride;
import software.amazon.awscdk.services.iam.PolicyStatement;
import software.amazon.awscdk.services.iam.Effect;
import software.amazon.awscdk.services.iam.ServicePrincipal;
import software.amazon.awscdk.services.iam.ArnPrincipal;

import software.amazon.awscdk.services.stepfunctions.tasks.TaskEnvironmentVariable;
import software.amazon.awscdk.services.stepfunctions.JsonPath;


import software.amazon.awscdk.services.ec2.Vpc;
import software.amazon.awscdk.services.ec2.InstanceType;
import software.amazon.awscdk.services.ec2.SecurityGroup;
import software.amazon.awscdk.services.ec2.Peer;
import software.amazon.awscdk.services.ec2.Port;

import java.util.HashMap;
import java.util.Map;
import java.util.List;
import java.util.Date;
import java.text.SimpleDateFormat;

class EcsStack extends Stack {
    private String dnsAddress;
    private EcsRunTask ecsTask;
    private Function exportLogsLambda;
    private Cluster cluster;
    private static final String bucketName = "BUCKET_NAME";
    private static final String logGroupName = "LOG_GROUP_NAME";


    public EcsStack(final Construct parent, final String id, final EcsStackProps props) {
        super(parent, id, props);

        String stackType = props.getStackType();
        String instanceType = props.getInstanceType();
        Vpc vpc = props.getVpc();
        Bucket bucket = props.getBucket();

        SecurityGroup sg = SecurityGroup.Builder.create(this, stackType + "ecs-service-sg")
            .vpc(vpc)
            .build();
        sg.addIngressRule(Peer.anyIpv4(), Port.allTraffic());

        cluster = Cluster.Builder.create(this, stackType + "-cluster")
            .vpc(vpc)
            .build();
        
        EcsOptimizedImage ecsMachineImage;
        if (props.getArm().equals("true")) {
            ecsMachineImage = EcsOptimizedImage.amazonLinux2(AmiHardwareType.ARM);
        } else {
            ecsMachineImage = EcsOptimizedImage.amazonLinux2();
        }
        
        AutoScalingGroup asg = AutoScalingGroup.Builder.create(this, stackType + "-asg")
            .vpc(vpc)
            .instanceType(new InstanceType(instanceType))
            .machineImage(ecsMachineImage)
            .minCapacity(0)
            .desiredCapacity(1)
            .securityGroup(sg)
            .build();

        AsgCapacityProvider asgProvider = AsgCapacityProvider.Builder.create(this, stackType + "-asg-provider")
            .autoScalingGroup(asg)
            .enableManagedTerminationProtection(false)
            .enableManagedScaling(false)
            .build();
        
        cluster.addAsgCapacityProvider(asgProvider);
        cluster.applyRemovalPolicy(RemovalPolicy.DESTROY);

        Ec2TaskDefinition task = Ec2TaskDefinition.Builder
            .create(this, stackType + "-task")
            .networkMode(NetworkMode.AWS_VPC)
            .build();

        Map<String, String> ecrEnv = new HashMap<>();
        ecrEnv.put("SCENARIO", props.getScenario());
        ecrEnv.put("PORT", "3000");  //Arbitrary port

        if (stackType.equals("server")) {
            PrivateDnsNamespace ecsNameSpace = PrivateDnsNamespace.Builder.create(this, stackType + "-namespace")
                .name(stackType + "ecs.com") //Arbitrary name
                .vpc(vpc)
                .build();

            LogGroup serviceLogGroup = LogGroup.Builder.create(this, "server-log-group")
                .retention(RetentionDays.ONE_DAY)
                .logGroupName("server-logs" + new SimpleDateFormat("MM-dd-yyyy-HH-mm-ss").format(new Date()).toString())
                .removalPolicy(RemovalPolicy.DESTROY)
                .build();

            bucket.grantPut(new ServicePrincipal("logs." + props.getServerRegion() + ".amazonaws.com"));
            bucket.addToResourcePolicy(PolicyStatement.Builder.create()
                .effect(Effect.ALLOW)
                .actions(List.of("s3:GetBucketAcl"))
                .principals(List.of(new ServicePrincipal("logs." + props.getServerRegion() + ".amazonaws.com")))
                .resources(List.of(bucket.getBucketArn()))
                .build());

            Map<String, String> exportLambdaLogsEnv = new HashMap<>();
            exportLambdaLogsEnv.put(bucketName, bucket.getBucketName());
            exportLambdaLogsEnv.put(logGroupName, serviceLogGroup.getLogGroupName());


            exportLogsLambda = Function.Builder.create(this, "export-logs-lambda")
                .runtime(Runtime.NODEJS_14_X)
                .handler("exportS3.handler")
                .code(Code.fromAsset("lambda"))
                .environment(exportLambdaLogsEnv)
                .logRetention(RetentionDays.ONE_DAY)    //One day to prevent reaching log limit, can be adjusted accordingly
                .build();

            exportLogsLambda.addToRolePolicy(PolicyStatement.Builder.create()
                .actions(List.of("logs:CreateExportTask"))
                .effect(Effect.ALLOW)
                .resources(List.of(serviceLogGroup.getLogGroupArn()))
                .build());

            bucket.grantReadWrite(exportLogsLambda.getRole());

            serviceLogGroup.addToResourcePolicy(PolicyStatement.Builder.create()
                    .actions(List.of("logs:CreateExportTask"))
                    .effect(Effect.ALLOW)
                    .principals(List.of(new ArnPrincipal(exportLogsLambda.getRole().getRoleArn())))
                    .build());
                    
            task.addContainer(stackType + "-driver", ContainerDefinitionOptions.builder()
                .image(ContainerImage.fromRegistry(props.getEcrUri()))
                .environment(ecrEnv)
                .memoryLimitMiB(2048)
                .logging(LogDriver.awsLogs(AwsLogDriverProps.builder().logGroup(serviceLogGroup).streamPrefix(stackType + "-ecs-task").build()))
                .portMappings(List.of(PortMapping.builder().containerPort(3000).hostPort(3000)
                    .protocol(software.amazon.awscdk.services.ecs.Protocol.UDP).build()))
                .build());

            bucket.grantWrite(task.getTaskRole());

            CloudMapOptions ecsServiceDiscovery = CloudMapOptions.builder()
                    .dnsRecordType(DnsRecordType.A)
                    .cloudMapNamespace(ecsNameSpace)
                    .name("ec2serviceserverCloudmapSrv-UEyneXTpp1nx") //Arbitrary hard-coded value to make DNS resolution easier
                    .build();
            
            dnsAddress = ecsServiceDiscovery.getName();

            Ec2Service service = Ec2Service.Builder.create(this, "ec2service-" + stackType)
                .cluster(cluster)
                .taskDefinition(task)
                .cloudMapOptions(ecsServiceDiscovery)
                .capacityProviderStrategies(List.of(CapacityProviderStrategy.builder()
                    .capacityProvider(asgProvider.getCapacityProviderName())
                    .weight(1)
                    .build()))
                .desiredCount(1)
                .securityGroups(List.of(sg))
                .build();
        } else {
            ecrEnv.put("DNS_ADDRESS", props.getDnsAddress() + ".serverecs.com");
            ecrEnv.put("SERVER_PORT", "3000");
            ecrEnv.put("S3_BUCKET", bucket.getBucketName());
            ecrEnv.put("LOCAL_IP", "0.0.0.0");

            ContainerDefinition clientContainer = task.addContainer(stackType + "-driver", ContainerDefinitionOptions.builder()
                .image(ContainerImage.fromRegistry(props.getEcrUri()))
                .environment(ecrEnv)
                .memoryLimitMiB(2048)
                .logging(LogDriver.awsLogs(AwsLogDriverProps.builder().logRetention(RetentionDays.ONE_DAY).streamPrefix(stackType + "-ecs-task").build()))
                .portMappings(List.of(PortMapping.builder().containerPort(3000).hostPort(3000)
                    .protocol(software.amazon.awscdk.services.ecs.Protocol.UDP).build()))
                .build()); 

            bucket.grantWrite(task.getTaskRole());

            ecsTask = EcsRunTask.Builder.create(this, "client-run-task")
                .integrationPattern(IntegrationPattern.RUN_JOB)
                .cluster(cluster)
                .taskDefinition(task)
                .launchTarget(EcsEc2LaunchTarget.Builder.create().build())
                .inputPath("$.Payload")
                .resultPath("$.client_result")
                .containerOverrides(List.of(ContainerOverride.builder()
                .containerDefinition(clientContainer)
                .environment(List.of(TaskEnvironmentVariable.builder()
                    .name("TIMESTAMP")
                    .value(JsonPath.stringAt("$.timestamp"))
                    .build()))
                .build()))
                .build();
        }
    }

    public String getDnsAddress() {
        return dnsAddress;
    }

    public EcsRunTask getEcsTask() {
        return ecsTask;
    }

    public Function getLogsLambda() {
        return exportLogsLambda;
    }

    public Cluster getCluster() {
        return cluster;
    }

}