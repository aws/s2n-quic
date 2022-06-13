// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.amazon.awscdk.Stack;
import software.amazon.awscdk.StackProps;
import software.amazon.awscdk.Environment;
import software.amazon.awscdk.services.ec2.Vpc;

public interface PeeringStackProps extends StackProps {

    public static Builder builder() {
        return new Builder();
    }

    Vpc getVpcClient();

    Vpc getVpcServer();

    public static class Builder {
        private Vpc VpcClient;
        private Vpc VpcServer;
        private Environment env;

        public Builder VpcClient(Vpc VpcClient) {
            this.VpcClient = VpcClient;
            return this;
        }

        public Builder VpcServer(Vpc VpcServer) {
            this.VpcServer = VpcServer;
            return this;
        }

        public Builder env(Environment env) {
            this.env = env;
            return this;
        }

        public PeeringStackProps build() {
            return new PeeringStackProps() {
                @Override
                public Vpc getVpcClient() {
                    return VpcClient;
                }

                @Override
                public Vpc getVpcServer() {
                    return VpcServer;
                }

                @Override
                public Environment getEnv() {
                    return env;
                }
            };
        }
    }
}