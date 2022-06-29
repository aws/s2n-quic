// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.amazon.awscdk.Stack;
import software.amazon.awscdk.StackProps;
import software.amazon.awscdk.services.s3.Bucket;
import software.amazon.awscdk.Environment;

public interface ClientServerStackProps extends StackProps {

    public static Builder builder() {
        return new Builder();
    }

    Bucket getBucket();

    String getInstanceType();

    String getCidr();

    String getStackType();

    String getProtocol();

    public static class Builder {
        private Bucket bucket;
        private String instanceType;
        private Environment env;
        private String cidr;
        private String stackType;
        private String protocol;

        public Builder bucket(Bucket bucket) {
            this.bucket = bucket;
            return this;
        }

        public Builder instanceType(String instanceType) {
            this.instanceType = instanceType;
            return this;
        }

        public Builder cidr(String cidr) {
            this.cidr = cidr;
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

        public Builder protocol(String protocol) {
            this.protocol = protocol;
            return this;
        }

        public ClientServerStackProps build() {
            return new ClientServerStackProps() {
                @Override
                public Bucket getBucket() {
                    return bucket;
                }

                @Override
                public String getInstanceType() {
                    return instanceType;
                }

                @Override
                public String getCidr() {
                    return cidr;
                }

                @Override
                public String getStackType() {
                    return stackType;
                }

                @Override
                public Environment getEnv() {
                    return env;
                }

                @Override
                public String getProtocol() {
                    return protocol;
                }
            };
        }
    }
}