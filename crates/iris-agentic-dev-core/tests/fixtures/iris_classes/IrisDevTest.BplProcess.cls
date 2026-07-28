Class IrisDevTest.BplProcess Extends Ens.BusinessProcessBPL
{

XData BPL [ XMLNamespace = "http://www.intersystems.com/bpl" ]
{
<process language='objectscript' request='Ens.Request' response='Ens.Response' >
<sequence>
<code name='DynamicDispatch' >
<![CDATA[
 Set tClass = "IrisDevTest.SomeTarget"
 Set status = $classmethod(tClass,"Process",request)
]]>
</code>
<call name='CallDownstream' target='DownstreamService' async='1' >
<request type='Ens.Request' />
<response type='Ens.Response' />
</call>
</sequence>
</process>
}

Storage Default
{
<Type>%Storage.Persistent</Type>
}

}
