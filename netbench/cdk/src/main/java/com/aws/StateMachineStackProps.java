// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.amazon.awscdk.StackProps;
import software.amazon.awscdk.Environment;
import software.amazon.awscdk.services.stepfunctions.tasks.EcsRunTask;
import software.amazon.awscdk.services.s3.Bucket;
import software.amazon.awscdk.services.lambda.Function;
import software.amazon.awscdk.services.ecs.Cluster;

public interface StateMachineStackProps extends StackProps {

    public static Builder builder() {
        return new Builder();
    }

    EcsRunTask getClientTask();

    Bucket getBucket();

    Function getLogsLambda();

    Cluster getCluster();

    String getProtocol();

    public static class Builder {
        private EcsRunTask clientTask;
        private Environment env;
        private Bucket bucket;
        private Function logsLambda;
        private Cluster cluster;
        private String protocol;

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

        public Builder logsLambda(Function logsLambda) {
            this.logsLambda = logsLambda;
            return this;
        }

        public Builder cluster(Cluster cluster) {
            this.cluster = cluster;
            return this;
        }

        public Builder protocol(String protocol) {
            this.protocol = protocol;
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
                public Function getLogsLambda() {
                    return logsLambda;
                }

                @Override
                public Cluster getCluster() {
                    return cluster;
                }

                @Override
                public String getProtocol() {
                    return protocol;
                }
            };
        }
    }
}