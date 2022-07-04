// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.amazon.awscdk.Stack;
import software.amazon.awscdk.StackProps;
import software.amazon.awscdk.Environment;

public interface ClientServerStackProps extends StackProps {

    public static Builder builder() {
        return new Builder();
    }

    String getCidr();

    String getStackType();

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

        public Builder stackType(String stackType) {
            this.stackType = stackType;
            return this;
        }

        public ClientServerStackProps build() {
            return new ClientServerStackProps() {

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
            };
        }
    }
}