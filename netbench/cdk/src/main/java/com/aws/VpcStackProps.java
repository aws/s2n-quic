// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.amazon.awscdk.Environment;
import software.amazon.awscdk.StackProps;

public interface VpcStackProps extends StackProps {

    public static Builder builder() {
        return new Builder();
    }

    String getCidr();

    public static class Builder {
        private Environment env;
        private String cidr;
        private String stackType;

        public Builder cidr(String cidr) {
            this.cidr = cidr;
            return this;
        }

        public Builder env(Environment env) {
            this.env = env;
            return this;
        }

        public VpcStackProps build() {
            return new VpcStackProps() {

                @Override
                public String getCidr() {
                    return cidr;
                }

                @Override
                public Environment getEnv() {
                    return env;
                }
            };
        }
    }
}