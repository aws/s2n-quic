// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.amazon.awscdk.Environment;
import software.amazon.awscdk.StackProps;
import software.amazon.awscdk.services.stepfunctions.tasks.EcsRunTask;

public interface StateMachineStackProps extends StackProps {

    public static Builder builder() {
        return new Builder();
    }

    EcsRunTask getClientTask();

    public static class Builder {
        private EcsRunTask clientTask;
        private Environment env;

        public Builder clientTask(EcsRunTask clientTask) {
            this.clientTask = clientTask;
            return this;
        }

        public Builder env(Environment env) {
            this.env = env;
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
            };
        }
    }
}