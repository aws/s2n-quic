// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.amazon.awscdk.Stack;
import software.amazon.awscdk.StackProps;
import software.amazon.awscdk.services.s3.Bucket;
import software.amazon.awscdk.Environment;

public interface ClientStackProps extends StackProps {

    public static Builder builder() {
        return new Builder();
    }

    Bucket getBucket();
    
    String getInstanceType();

    public static class Builder {
        private Bucket bucket;
        private String instanceType;
        private Environment env;

        public Builder bucket(Bucket bucket) {
            this.bucket = bucket;
            return this;
        }

        public Builder instanceType(String instanceType) {
            this.instanceType = instanceType;
            return this;
        }

        public Builder env(Environment env) {
            this.env = env;
            return this;
        }

        public ClientStackProps build() {
            return new ClientStackProps() {
                @Override
                public Bucket getBucket() {
                    return bucket;
                }

                @Override
                public String getInstanceType() {
                    return instanceType;
                }

                @Override
                public Environment getEnv() {
                    return env;
                }
            };
        }
    }
}