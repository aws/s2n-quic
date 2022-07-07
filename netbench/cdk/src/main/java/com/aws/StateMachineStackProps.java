// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.amazon.awscdk.Stack;
import software.amazon.awscdk.StackProps;
import software.amazon.awscdk.Environment;
import software.amazon.awscdk.services.stepfunctions.tasks.EcsRunTask;
import software.amazon.awscdk.services.s3.Bucket;
import software.amazon.awscdk.services.logs.LogGroup;
import software.amazon.awscdk.services.lambda.Function;

public interface StateMachineStackProps extends StackProps {

    public static Builder builder() {
        return new Builder();
    }

    EcsRunTask getClientTask();

    Bucket getBucket();

    String getRegion();

    String getLogGroupName();

    LogGroup getServiceLogGroup();

    Function getLogsLambda();

    public static class Builder {
        private EcsRunTask clientTask;
        private Environment env;
        private Bucket bucket;
        private String region;
        private String logGroupName;
        private LogGroup serviceLogGroup;
        private Function logsLambda;

        public Builder clientTask(EcsRunTask clientTask) {
            this.clientTask = clientTask;
            return this;
        }

        public Builder env(Environment env) {
            this.env = env;
            return this;
        }

        public Builder bucket(Bucket bucket) {
            this.bucket = bucket;
            return this;
        }

        public Builder region(String region) {
            this.region = region;
            return this;
        }

        public Builder logGroupName(String logGroupName) {
            this.logGroupName = logGroupName;
            return this;
        }
        
        public Builder serviceLogGroup(LogGroup serviceLogGroup) {
            this.serviceLogGroup = serviceLogGroup;
            return this;
        }

        public Builder logsLambda(Function logsLambda) {
            this.logsLambda = logsLambda;
            return this;
        }

        public StateMachineStackProps build() {
            return new StateMachineStackProps() {

                @Override
                public EcsRunTask getClientTask() {
                    return clientTask;
                }

                @Override
                public Environment getEnv() {
                    return env;
                }

                @Override
                public Bucket getBucket() {
                    return bucket;
                }

                @Override
                public String getRegion() {
                    return region;
                }

                @Override
                public String getLogGroupName() {
                    return logGroupName;
                }

                @Override
                public LogGroup getServiceLogGroup() {
                    return serviceLogGroup;
                }

                @Override
                public Function getLogsLambda() {
                    return logsLambda;
                }
            };
        }
    }
}