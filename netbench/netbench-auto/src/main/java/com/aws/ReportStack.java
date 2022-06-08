package com.aws;

import software.constructs.Construct;
import software.amazon.awscdk.Stack;
import software.amazon.awscdk.StackProps;
import software.amazon.awscdk.services.s3.Bucket;
import software.amazon.awscdk.RemovalPolicy;
import software.amazon.awscdk.Environment;


public class ReportStack extends Stack {
    private Bucket metricsBucket;

    public ReportStack(final Construct parent, final String id) {
        this(parent, id, null);
    }

    public ReportStack(final Construct parent, final String id, final StackProps props) {
        super(parent, id, props);

        //Environment env = props.getEnv();
        metricsBucket = Bucket.Builder.create(this, "MetricsReportBucket")
            .removalPolicy(RemovalPolicy.DESTROY)
            .autoDeleteObjects(true)
            .build();
    }

    public Bucket getBucket() {
        return metricsBucket;
    }
}                                         

