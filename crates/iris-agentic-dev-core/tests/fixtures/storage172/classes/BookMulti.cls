Class IadSm172.BookMulti Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property Title As %String;

Property Publisher As %String;

Property Location As %String;

Storage SQLStorage
{
<SqlIdExpression>$i(^IadSm172BookMulti)</SqlIdExpression>
<SQLMap name="Master">
<Data name="Location">
<Node>"loc"</Node>
</Data>
<Data name="Publisher">
<Node>"pub"</Node>
</Data>
<Data name="Title">
<Node>"title"</Node>
</Data>
<Global>^IadSm172BookMulti</Global>
<Subscript name="1">
<Expression>{ID}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172BookMultiS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}

