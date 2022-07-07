// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.constructs.Construct;
import software.amazon.awscdk.Stack;
import software.amazon.awscdk.StackProps;
import software.amazon.awscdk.services.stepfunctions.tasks.EcsRunTask;
import software.amazon.awscdk.services.stepfunctions.StateMachine;
import software.amazon.awscdk.services.s3.Bucket;
import software.amazon.awscdk.services.ecr.Repository;
import software.amazon.awscdk.Size;
import software.amazon.awscdk.services.logs.RetentionDays;
import software.amazon.awscdk.Duration;

import software.amazon.awscdk.services.lambda.DockerImageFunction;
import software.amazon.awscdk.services.lambda.DockerImageCode;
import software.amazon.awscdk.services.lambda.Function;
import software.amazon.awscdk.services.lambda.Architecture;
import software.amazon.awscdk.services.lambda.Runtime;
import software.amazon.awscdk.services.lambda.Code;

import software.amazon.awscdk.services.stepfunctions.tasks.LambdaInvoke;
import software.amazon.awscdk.services.stepfunctions.tasks.LambdaInvocationType;
import software.amazon.awscdk.services.stepfunctions.Wait;
import software.amazon.awscdk.services.stepfunctions.WaitTime;

import software.amazon.awscdk.services.logs.LogGroup;
import software.amazon.awscdk.services.logs.RetentionDays;

import software.amazon.awscdk.services.iam.PolicyStatement;
import software.amazon.awscdk.services.iam.Effect;
import software.amazon.awscdk.services.iam.AnyPrincipal;
import software.amazon.awscdk.services.iam.ServicePrincipal;
import software.amazon.awscdk.services.iam.ArnPrincipal;

import java.util.HashMap;
import java.util.List;

public class StateMachineStack extends Stack {

    public StateMachineStack(final Construct parent, final String id, final StateMachineStackProps props) {
        super(parent, id, props);

        Bucket metricsBucket = props.getBucket();

        /*
        HashMap lambdaEnv = new HashMap<>();
        lambdaEnv.put("S3_BUCKET", metricsBucket.getBucketName());

        DockerImageFunction reportGenerationLambda = DockerImageFunction.Builder.create(this, "report-generation-lambda")
            .environment(lambdaEnv)
            .ephemeralStorageSize(Size.mebibytes(2048))
            .code(DockerImageCode.fromEcr(Repository.fromRepositoryName(this, "report-image", "netbench-cli")))
            .logRetention(RetentionDays.ONE_DAY)
            .memorySize(2048)
            .architecture(Architecture.ARM_64)
            .retryAttempts(0)
            .timeout(Duration.minutes(1))
            .build();
        
        metricsBucket.grantReadWrite(reportGenerationLambda.getRole());
        */
        LambdaInvoke exportsLogsLambdaInvoke = LambdaInvoke.Builder.create(this, "export-logs-task")
                .lambdaFunction(props.getLogsLambda())
                .invocationType(LambdaInvocationType.REQUEST_RESPONSE)
                .build();

        Wait waitFunction = Wait.Builder.create(this, "wait-step")
            .time(WaitTime.duration(Duration.seconds(60)))
            .build();
        
        waitFunction.next(props.getClientTask());

        exportsLogsLambdaInvoke.next(waitFunction);

        StateMachine stateMachine = StateMachine.Builder.create(this, "ecs-state-machine")
            //.definition(props.getClientTask())
            //.definition(LambdaInvoke.Builder.create(this, "lambda-task")
            //    .lambdaFunction(reportGenerationLambda)
            //    .build())
            .definition(exportsLogsLambdaInvoke)
            .build();
        
    }
}                                         

