// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
package com.aws;

import software.constructs.Construct;
import software.amazon.awscdk.Stack;
import software.amazon.awscdk.StackProps;
import software.amazon.awscdk.services.s3.Bucket;
import software.amazon.awscdk.RemovalPolicy;
import software.amazon.awscdk.Environment;
import software.amazon.awscdk.PhysicalName;


public class ReportStack extends Stack {
    private Bucket metricsBucket;

    public ReportStack(final Construct parent, final String id) {
        this(parent, id, null);
    }

    public ReportStack(final Construct parent, final String id, final StackProps props) {
        super(parent, id, props);

        metricsBucket = Bucket.Builder.create(this, "MetricsReportBucket")
            .bucketName(PhysicalName.GENERATE_IF_NEEDED)
            .removalPolicy(RemovalPolicy.DESTROY)
            .autoDeleteObjects(true)
            .build();
    }

    public Bucket getBucket() {
        return metricsBucket;
    }
}                                         

