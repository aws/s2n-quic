package com.aws;

import software.amazon.awscdk.App;
import software.amazon.awscdk.assertions.Template;
import java.io.IOException;
import org.junit.jupiter.api.Test;
import java.util.HashMap;
import software.amazon.awscdk.Environment;
import java.util.List;
import java.util.Map;


/* Unit testing for ServerEcsStack, tests for key properties
 * in the cloudformation output of the stack.
 * Some empty maps and lists can be found to match
 * any value for fields in the Cloudformation output with
 * unimportant information.
 * More extensive tests are performed through deployments
 * of various scenarios and verifying outputs.
 */
public class ServerEcsStackTest {
    private String cidr;
    private String region;
    private String instanceType;
    private String ecrUri;
    private String scenarioFile;
    private Template serverTemplate;
    private Template vpcTemplate;
    private EcsStack serverEcsStack;
    private VpcStack vpcStack;
    private String arm;

    public ServerEcsStackTest() {
        App app = new App();

        cidr = "11.0.0.0/16";
        region = "us-west-2";
        instanceType = "t4g.xlarge";
        ecrUri = "public.ecr.aws/d2r9y8c2/s2n-quic-collector-server-scenario";
        scenarioFile = "/usr/bin/request_response.json";
        arm = "true";


        vpcStack = new VpcStack(app, "VpcStack", VpcStackProps.builder()
            .env(makeEnv(System.getenv("CDK_DEFAULT_ACCOUNT"), region))
            .cidr(cidr)
            .build());

        serverEcsStack = new EcsStack(app, "ServerEcsStack", EcsStackProps.builder()
            .env(makeEnv(System.getenv("CDK_DEFAULT_ACCOUNT"), region))
            .bucket(vpcStack.getBucket())
            .stackType("server")
            .vpc(vpcStack.getVpc())
            .instanceType(instanceType)
            .ecrUri(ecrUri)
            .scenario(scenarioFile)
            .serverRegion(region)
            .arm(arm)
            .build());
        
        serverTemplate = Template.fromStack(serverEcsStack);
                
        vpcTemplate = Template.fromStack(vpcStack);
    }

    @Test
     public void testServerStack() throws IOException {
        
        //Security Group
        serverTemplate.hasResourceProperties("AWS::EC2::SecurityGroup", new HashMap<String, Object>() {{
            put("SecurityGroupEgress", List.of(Map.of("CidrIp", "0.0.0.0/0", "Description", "Allow all outbound traffic by default","IpProtocol", "-1")));
            put("SecurityGroupIngress", List.of(Map.of("CidrIp", "0.0.0.0/0", "Description", "from 0.0.0.0/0:ALL TRAFFIC","IpProtocol", "-1")));
        }});

        //Cluster
        serverTemplate.hasResource("AWS::ECS::Cluster", new HashMap<String, Object>() {{
            put("DeletionPolicy", "Delete");
        }});

        //Launch config
        serverTemplate.hasResourceProperties("AWS::AutoScaling::LaunchConfiguration", new HashMap<String, Object>() {{
            put("InstanceType", instanceType);
        }});
        
        //Autoscaling group
        serverTemplate.hasResourceProperties("AWS::AutoScaling::AutoScalingGroup", new HashMap<String, Object>() {{
            put("DesiredCapacity", "1");
            put("MinSize", "0");
        }});

        //Asg Provider
        serverTemplate.hasResourceProperties("AWS::ECS::CapacityProvider", new HashMap<String, Object>() {{
            put("AutoScalingGroupProvider", Map.of("ManagedTerminationProtection","DISABLED"));
        }});

        //Task Definition
        serverTemplate.hasResourceProperties("AWS::ECS::TaskDefinition", new HashMap<String, Object>() {{
            put("ContainerDefinitions", List.of(Map.of(
                "Environment", List.of(Map.of("Name", "PORT", "Value", "3000"), Map.of("Name", "SCENARIO", "Value", scenarioFile)),
                "PortMappings", List.of(Map.of("ContainerPort", 3000, "HostPort", 3000, "Protocol", "udp")),
                "Image", ecrUri,
                "LogConfiguration", Map.of("LogDriver", "awslogs", "Options", Map.of( 
                "awslogs-stream-prefix", "server-ecs-task")))
            ));
            put("NetworkMode", "awsvpc");
        }});

        //Server task policy to access metrics s3 bucket
        serverTemplate.hasResourceProperties("AWS::IAM::Policy", new HashMap<String, Object>() {{
            put("PolicyDocument", Map.of("Statement", 
            List.of(Map.of("Action", 
                List.of(
                "s3:DeleteObject*",
                "s3:PutObject",
                "s3:PutObjectLegalHold",
                "s3:PutObjectRetention",
                "s3:PutObjectTagging",
                "s3:PutObjectVersionTagging",
                "s3:Abort*"),
                "Effect", "Allow",
                "Resource", List.of(Map.of(), Map.of()))),
                "Version", "2012-10-17"));
        }});
        
        //Server task policy can send logs to Cloudwatch
        serverTemplate.hasResourceProperties("AWS::IAM::Policy", new HashMap<String, Object>() {{
            put("PolicyDocument", Map.of("Statement", 
            List.of(Map.of("Action", 
                List.of(
                "logs:CreateLogStream",
                "logs:PutLogEvents"),
                "Effect", "Allow",
                "Resource", Map.of()))));
        }});

        //Namespace
        serverTemplate.hasResourceProperties("AWS::ServiceDiscovery::PrivateDnsNamespace", new HashMap<String, Object>() {{
            put("Name", "serverecs.com");
        }});

        //LogGroup
        serverTemplate.hasResource("AWS::Logs::LogGroup", new HashMap<String, Object>() {{
            put("Properties", Map.of("RetentionInDays", 1));
        }});
        
        //Export lambda function
        serverTemplate.hasResourceProperties("AWS::Lambda::Function", new HashMap<String, Object>() {{
            put("Handler", "exportS3.handler");
            put("Runtime", "nodejs14.x");
        }});

        //Export lambda policy permissions
        serverTemplate.hasResourceProperties("AWS::IAM::Policy", new HashMap<String, Object>() {{
            put("PolicyDocument", Map.of("Statement", List.of(
                Map.of("Action", "logs:CreateExportTask", "Effect", "Allow"), 
                Map.of("Action", List.of("s3:GetObject*",
                "s3:GetBucket*",
                "s3:List*",
                "s3:DeleteObject*",
                "s3:PutObject",
                "s3:PutObjectLegalHold",
                "s3:PutObjectRetention",
                "s3:PutObjectTagging",
                "s3:PutObjectVersionTagging",
                "s3:Abort*"),
                "Effect", "Allow")
            )));
        }});

        //Log Retention
        serverTemplate.hasResourceProperties("Custom::LogRetention", new HashMap<String, Object>() {{
            put("RetentionInDays", 1);
        }});

        //ECS Service
        serverTemplate.hasResourceProperties("AWS::ECS::Service", new HashMap<String, Object>() {{
            put("CapacityProviderStrategy", List.of(Map.of("CapacityProvider", Map.of())));
            put("DesiredCount", 1);
            put("NetworkConfiguration", Map.of("AwsvpcConfiguration", Map.of()));
        }});

        //Service Discovery
        serverTemplate.hasResourceProperties("AWS::ServiceDiscovery::Service", new HashMap<String, Object>() {{
            put("DnsConfig", Map.of("DnsRecords", List.of(Map.of("TTL", 60, "Type", "A"))));
            put ("Name", "ec2serviceserverCloudmapSrv-UEyneXTpp1nx");
        }});

        //Check bucket policy changed by export lambda
        vpcTemplate.hasResourceProperties("AWS::S3::BucketPolicy", new HashMap<String, Object>() {{
            put("PolicyDocument", Map.of( "Statement", List.of(
              Map.of("Action", List.of( "s3:PutObject","s3:PutObjectLegalHold",
                "s3:PutObjectRetention","s3:PutObjectTagging", "s3:PutObjectVersionTagging","s3:Abort*"),
                  "Effect", "Allow", "Principal", Map.of("Service", "logs.us-west-2.amazonaws.com")),
              Map.of("Action", "s3:GetBucketAcl", "Effect", "Allow", "Principal", Map.of("Service", "logs.us-west-2.amazonaws.com"))
              )));
          }});
     }

     static Environment makeEnv(String account, String region) {
      return Environment.builder()
          .account(account)
          .region(region)
          .build();
  }
}
