// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.amazon.awscdk.StackProps;
import software.amazon.awscdk.Environment;
import software.amazon.awscdk.StackProps;
import software.amazon.awscdk.services.ec2.Vpc;

public interface PeeringStackProps extends StackProps {

    static Builder builder() {
        return new Builder();
    }

    Vpc getVpcClient();

    Vpc getVpcServer();

    String getStackType();

    String getRef();

    String getCidr();

    String getRegion();

    class Builder {
        private Vpc vpcClient;
        private Vpc vpcServer;
        private Environment env;
        private String stackType;
        private String ref;
        private String cidr;
        private String region;

        public Builder VpcClient(Vpc vpcClient) {
            this.vpcClient = vpcClient;
            return this;
        }

        public Builder VpcServer(Vpc vpcServer) {
            this.vpcServer = vpcServer;
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
                    return vpcClient;
                }

                @Override
                public Vpc getVpcServer() {
                    return vpcServer;
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