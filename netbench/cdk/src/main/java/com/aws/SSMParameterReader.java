
// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.amazon.awscdk.customresources.AwsCustomResource;
import software.constructs.Construct;

class SSMParameterReader extends AwsCustomResource {
    public SSMParameterReader(final Construct scope, final String id, 
        final SSMParameterReaderProps props) {
            
        super(scope, id, props);
    }

    public String getParameterValue() {
        return this.getResponseField("Parameter.Value").toString();
    }
}