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

    String getStackType();

    String getRef();

    String getCidr();

    String getRegion();

    public static class Builder {
        private Vpc VpcClient;
        private Vpc VpcServer;
        private Environment env;
        private String stackType;
        private String ref;
        private String cidr;
        private String region;

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

        public Builder stackType(String stackType) {
            this.stackType = stackType;
            return this;
        }

        public Builder ref(String ref) {
            this.ref = ref;
            return this;
        }

        public Builder cidr(String cidr) {
            this.cidr = cidr;
            return this;
        }

        public Builder region(String region) {
            this.region = region;
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

                @Override
                public String getStackType() {
                    return stackType;
                }

                @Override
                public String getRef() {
                    return ref;
                }

                @Override
                public String getCidr() {
                    return cidr;
                }

                @Override
                public String getRegion() {
                    return region;
                }
            };
        }
    }
}