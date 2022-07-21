// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.constructs.Construct;
import software.amazon.awscdk.Stack;
import software.amazon.awscdk.services.stepfunctions.tasks.EcsRunTask;
import software.amazon.awscdk.services.stepfunctions.StateMachine;
import software.amazon.awscdk.services.s3.Bucket;
import software.amazon.awscdk.Duration;
import software.amazon.awscdk.services.logs.RetentionDays;

import software.amazon.awscdk.services.stepfunctions.tasks.LambdaInvoke;
import software.amazon.awscdk.services.stepfunctions.tasks.LambdaInvocationType;
import software.amazon.awscdk.services.stepfunctions.Wait;
import software.amazon.awscdk.services.stepfunctions.WaitTime;
import software.amazon.awscdk.services.stepfunctions.tasks.ContainerOverride;
import software.amazon.awscdk.services.stepfunctions.tasks.TaskEnvironmentVariable;
import software.amazon.awscdk.services.stepfunctions.JsonPath;
import software.amazon.awscdk.services.stepfunctions.IntegrationPattern;
import software.amazon.awscdk.services.stepfunctions.tasks.EcsEc2LaunchTarget;

import software.amazon.awscdk.services.ecs.ContainerDefinition;
import software.amazon.awscdk.services.ecs.ContainerDefinitionOptions;
import software.amazon.awscdk.services.ecs.Ec2TaskDefinition;
import software.amazon.awscdk.services.ecs.ContainerImage;
import software.amazon.awscdk.services.ecs.AwsLogDriverProps;
import software.amazon.awscdk.services.ecs.LogDriver;

import java.util.HashMap;
import java.util.Map;
import java.util.List;

public class StateMachineStack extends Stack {

    public StateMachineStack(final Construct parent, final String id, final StateMachineStackProps props) {
        super(parent, id, props);

        Bucket bucket = props.getBucket();

        EcsRunTask clientTask = props.getClientTask();

        Wait waitFunction = Wait.Builder.create(this, "wait-step")
            .time(WaitTime.duration(Duration.seconds(20)))
            .build();

        Ec2TaskDefinition reportGenerationTask = Ec2TaskDefinition.Builder
            .create(this, "report-generation-task")
            .build();

        Map<String, String> reportGenerationEnv = new HashMap<>();
        reportGenerationEnv.put("S3_BUCKET", bucket.getBucketName());
        reportGenerationEnv.put("DRIVER", props.getDriver());

        ContainerDefinition reportGenerationContainer = reportGenerationTask.addContainer("report-generation", ContainerDefinitionOptions.builder()
            .image(ContainerImage.fromRegistry("public.ecr.aws/d2r9y8c2/netbench-cli"))
            .environment(reportGenerationEnv)
            .memoryLimitMiB(2048)
            .logging(LogDriver.awsLogs(AwsLogDriverProps.builder().logRetention(RetentionDays.ONE_DAY).streamPrefix("report-generation").build()))
            .build()); 

        bucket.grantReadWrite(reportGenerationTask.getTaskRole());

        EcsRunTask reportGenerationStep = EcsRunTask.Builder.create(this, "report-generation-step")
            .integrationPattern(IntegrationPattern.RUN_JOB)
            .cluster(props.getCluster())
            .taskDefinition(reportGenerationTask)
            .launchTarget(EcsEc2LaunchTarget.Builder.create().build())
            .inputPath("$.Payload")
            .containerOverrides(List.of(ContainerOverride.builder()
                .containerDefinition(reportGenerationContainer)
                .environment(List.of(TaskEnvironmentVariable.builder()
                    .name("EXPORT_TASK_ID")
                    .value(JsonPath.stringAt("$.body.taskId"))
                    .build()))
                .build()))
            .build();

        LambdaInvoke exportServerLogsLambdaInvoke = LambdaInvoke.Builder.create(this, "export-server-logs-task")
                .lambdaFunction(props.getLogsLambda())
                .invocationType(LambdaInvocationType.REQUEST_RESPONSE)
                .build();

        clientTask.next(exportServerLogsLambdaInvoke);

        exportServerLogsLambdaInvoke.next(waitFunction);
        
        waitFunction.next(reportGenerationStep);

        StateMachine stateMachine = StateMachine.Builder.create(this, "ecs-state-machine")
            .definition(clientTask)
            .build();
        
    }
}                                         

