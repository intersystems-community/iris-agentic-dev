Class IadSm172.Branch Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property BranchId As %String;

Property Country As %String;

Property BranchName As %String;

Storage SQLStorage
{
<SqlIdExpression>$i(^IadSm172Branch)</SqlIdExpression>
<SQLMap name="Master">
<Data name="BranchId">
<Delimiter>"^"</Delimiter>
<Piece>1</Piece>
</Data>
<Data name="BranchName">
<Delimiter>"^"</Delimiter>
<Piece>3</Piece>
</Data>
<Data name="Country">
<Delimiter>"^"</Delimiter>
<Piece>2</Piece>
</Data>
<Global>^IadSm172Branch</Global>
<Subscript name="1">
<Expression>{ID}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172BranchS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}

