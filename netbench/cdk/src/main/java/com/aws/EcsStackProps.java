// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.amazon.awscdk.StackProps;
import software.amazon.awscdk.services.s3.Bucket;
import software.amazon.awscdk.Environment;
import software.amazon.awscdk.services.ec2.Vpc;

public interface EcsStackProps extends StackProps {

    public static Builder builder() {
        return new Builder();
    }

    Bucket getBucket();

    String getInstanceType();

    String getStackType();

    String getProtocol();

    Vpc getVpc();

    String getServerRegion();

    String getDnsAddress();

    String getEcrUri();

    String getScenario();

    public static class Builder {
        private Bucket bucket;
        private String instanceType;
        private Environment env;
        private String stackType;
        private String protocol;
        private Vpc vpc;
        private String serverRegion;
        private String dnsAddress;
        private String ecrUri;
        private String scenario;

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

        public Builder stackType(String stackType) {
            this.stackType = stackType;
            return this;
        }

        public Builder protocol(String protocol) {
            this.protocol = protocol;
            return this;
        }

        public Builder vpc(Vpc vpc) {
            this.vpc = vpc;
            return this;
        }

        public Builder serverRegion(String serverRegion) {
            this.serverRegion = serverRegion;
            return this;
        }

        public Builder dnsAddress(String dnsAddress) {
            this.dnsAddress = dnsAddress;
            return this;
        }

        public Builder ecrUri(String ecrUri) {
            this.ecrUri = ecrUri;
            return this;
        }

        public Builder scenario(String scenario) {
            this.scenario = scenario;
            return this;
        }

        public EcsStackProps build() {
            return new EcsStackProps() {
                @Override
                public Bucket getBucket() {
                    return bucket;
                }

                @Override
                public String getInstanceType() {
                    return instanceType;
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

                @Override
                public Vpc getVpc() {
                    return vpc;
                }

                @Override
                public String getServerRegion() {
                    return serverRegion;
                }

                @Override
                public String getDnsAddress() {
                    return dnsAddress;
                }

                @Override
                public String getEcrUri() {
                    return ecrUri;
                }
                @Override
                public String getScenario() {
                    return scenario;
                }
            };
        }
    }
}