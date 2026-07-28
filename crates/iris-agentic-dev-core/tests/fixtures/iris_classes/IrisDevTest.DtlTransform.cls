Class IrisDevTest.DtlTransform Extends Ens.DataTransformDTL
{

Parameter IGNOREMISSINGSOURCE = 0;

Parameter REPORTERRORS = 0;

XData DTL [ XMLNamespace = "http://www.intersystems.com/dtl" ]
{
<transform sourceClass='Ens.Request' targetClass='Ens.Response' create='new' language='objectscript' >
<assign value='"a"' property='target.%ClassName(1)' action='set' />
<assign value='"b"' property='target.%ClassName(1)' action='set' />
<assign value='"c"' property='target.%ClassName(1)' action='set' />
</transform>
}

}
