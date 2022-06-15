// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.amazon.awscdk.customresources.AwsSdkCall;
import software.amazon.awscdk.customresources.SdkCallsPolicyOptions;
import software.amazon.awscdk.customresources.AwsCustomResource;
import software.amazon.awscdk.customresources.AwsCustomResourceProps;
import software.amazon.awscdk.customresources.AwsCustomResourcePolicy;
import software.amazon.awscdk.customresources.PhysicalResourceId;
import software.constructs.Construct;
import java.util.HashMap;

public interface SSMParameterReaderProps extends AwsCustomResourceProps {
    public static Builder builder() {
        return new Builder();
    }

    public static class Builder {
        private AwsSdkCall sdkCall;
        private AwsCustomResourcePolicy policy;

        public Builder sdkCall(String name, String region) {
            HashMap<String, String> sdkParameters = new HashMap<>();
            sdkParameters.put("Name", name);
            this.sdkCall = AwsSdkCall.builder()
                .service("SSM")
                .action("getParameter")
                .parameters(sdkParameters)
                .region(region)
                .physicalResourceId(PhysicalResourceId.of(String.valueOf(System.currentTimeMillis())))
                .build();
            return this;
        }

        public Builder policy() {
            this.policy = AwsCustomResourcePolicy.fromSdkCalls(SdkCallsPolicyOptions.builder()
                 .resources(AwsCustomResourcePolicy.ANY_RESOURCE)
                 .build());
            return this;
        }

        public SSMParameterReaderProps build() {
            return new SSMParameterReaderProps() {
                @Override
                public AwsSdkCall getOnUpdate() {
                    return sdkCall;
                }

                @Override
                public AwsCustomResourcePolicy getPolicy() {
                    return policy;
                }
            };
        }
    }
}